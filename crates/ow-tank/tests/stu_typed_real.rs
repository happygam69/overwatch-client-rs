//! The typed STU layer against the real installation: the registry loaded from
//! the reference checkout, and typed reads on the shapes real assets have —
//! a hero, the hero-settings asset, an entity's component map, a weapon
//! graph's volley motions, and the version-1 asset buckets — each cross-checked
//! against the hand-written readers where one exists.
//!
//! Skipped when no install is present, or when no STU class checkout is
//! configured (the typed layer is registry-driven).

use ow_stu::types::{DefaultValue, FieldType, Value};
use ow_tank::classreg::ClassRegistry as TankRegistry;
use ow_tank::keytable::Sources;
use ow_tank::{classes, guid, settings, volley, GameData, ManifestSelector};

/// Hero 2's asset, the reference hero the movement and view-model work uses.
const HERO_2: u64 = 0x02E0_0000_0000_0002;
/// The hero-settings asset this install ships (type `STU generic settings`,
/// index 294 on the reference install).
const HERO_SETTINGS_ASSET: u64 = 0x0CA0_0000_0000_0126;
/// `STUEntityDefinition` (found on every hero's entity asset).
const ENTITY_DEFINITION: u32 = 0x73BB_2F3A;
/// `m_componentMap` on `STUEntityDefinition`.
const COMPONENT_MAP_FIELD: u32 = 0x8AF8_F4F5;
/// How many volley states to cross-check against the raw reader.
const VOLLEY_CAP: usize = 50;
/// Upper bound on version-1 assets read off disk (each is a CASC hit; the scan
/// strides over the rest).
const V1_SCAN_CAP: usize = 1024;

fn open() -> Option<(GameData, TankRegistry)> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return None;
        }
    };
    let registry = TankRegistry::load_from_sources(&sources);
    if registry.is_empty() {
        eprintln!("note: no STU class checkout found; typed checks skipped");
        return None;
    }
    Some((data, registry))
}

#[test]
fn real_stu_typed_layer() {
    let Some((data, registry)) = open() else {
        return;
    };
    let reg = registry.inner();

    assert!(
        data.bytes_of(HERO_2).is_ok(),
        "hero 2 (0x{HERO_2:016x}) is not in the manifests; the rest can't run"
    );

    // -----------------------------------------------------------------------
    // The registry itself
    // -----------------------------------------------------------------------
    eprintln!(
        "registry: {} files, {} classes, {} enums, {} declared fields",
        registry.files,
        reg.class_count(),
        reg.enum_count(),
        reg.field_count()
    );
    assert!(reg.class_count() > 7000, "suspiciously few classes");
    assert!(reg.enum_count() > 500, "suspiciously few enums");
    assert!(reg.field_count() > 20000, "suspiciously few fields");
    // Every declared type should map; a leftover means the mapper needs a case,
    // not that the data is wrong.
    assert!(
        registry.unknown_types.is_empty(),
        "declared types the mapper does not know: {:?}",
        registry.unknown_types
    );
    assert!(
        registry.unknown_bases.is_empty(),
        "base classes that did not resolve: {:?}",
        registry.unknown_bases
    );
    // The generator's initializers are a closed set of forms; anything left
    // here is an unhandled form (the field is assembled with no default
    // instead, which the typed layer would treat as C# default(0)).
    assert!(
        registry.unparsed_defaults.is_empty(),
        "initializers the loader does not parse: {:?}",
        registry.unparsed_defaults
    );

    let gameplay_entity = reg
        .field(classes::HERO, classes::hero::GAMEPLAY_ENTITY)
        .expect("STUHero declares m_gameplayEntity");
    assert_eq!(gameplay_entity.ty, FieldType::AssetRef);

    let setting_max = reg
        .field(
            classes::HERO_SETTING_FLOAT,
            classes::hero_setting_float::MAX,
        )
        .expect("STUHeroSettingFloat declares m_max");
    assert_eq!(
        setting_max.default,
        DefaultValue::Float(1.0),
        "m_max's class default is 1.0f (the value settings.rs carries as \
         HERO_SETTING_FLOAT_DEFAULTS)"
    );
    let setting_min = reg
        .field(
            classes::HERO_SETTING_FLOAT,
            classes::hero_setting_float::MIN,
        )
        .expect("STUHeroSettingFloat declares m_min");
    assert_eq!(
        setting_min.default,
        DefaultValue::None,
        "m_min has no initializer: its stored default is the type's zero"
    );

    // -----------------------------------------------------------------------
    // A hero asset
    // -----------------------------------------------------------------------
    let mut hero_stu = data.stu_of(HERO_2).expect("hero 2 parses");
    assert!(!hero_stu.is_v1(), "hero 2 is a version 2 asset");
    let hero = hero_stu
        .instance(classes::HERO)
        .expect("hero 2 carries a STUHero instance");

    let entity_field = hero_stu
        .typed_field(hero, classes::hero::GAMEPLAY_ENTITY, reg)
        .expect("m_gameplayEntity is declared");
    let entity_guid = match entity_field.unwrap_defaulted() {
        Value::AssetRef(g) => *g,
        other => panic!("m_gameplayEntity: {other:?}"),
    };
    assert!(entity_guid != 0, "m_gameplayEntity is null");
    assert_eq!(
        guid::type_of(entity_guid),
        0x003,
        "m_gameplayEntity points at a non-entity 0x{entity_guid:016x}"
    );
    eprintln!("hero 2: gameplayEntity 0x{entity_guid:016x}");

    let gender = hero_stu
        .typed_field(hero, classes::hero::GENDER, reg)
        .expect("m_gender is declared");
    match gender.unwrap_defaulted() {
        Value::Enum { raw, member } => {
            eprintln!("hero 2: gender {member:?} (raw {raw})");
            assert!(*raw < 10, "m_gender raw {raw} is out of range");
        }
        other => panic!("m_gender: {other:?}"),
    }

    let color = hero_stu
        .typed_field(hero, classes::hero::HERO_COLOR, reg)
        .expect("m_heroColor is declared");
    match color.unwrap_defaulted() {
        Value::Struct(components) => {
            eprintln!("hero 2: heroColor {components:?}");
            assert_eq!(components.len(), 4, "teColorRGBA holds four floats");
        }
        other => panic!("m_heroColor: {other:?}"),
    }

    // -----------------------------------------------------------------------
    // Entity component maps, over a few hero entities
    // -----------------------------------------------------------------------
    let heroes = data.guids_of_type(guid::types::HERO);
    let mut sampled = 0usize;
    let mut with_health = 0usize;
    let mut entries_total = 0usize;
    let mut key_is_class = 0usize;
    for g in heroes.iter().filter(|g| guid::index_of(**g) <= 8) {
        let Ok(mut h) = data.stu_of(*g) else { continue };
        let Some(instance) = h.instance(classes::HERO) else {
            continue;
        };
        let Some(entity_field) = h.typed_field(instance, classes::hero::GAMEPLAY_ENTITY, reg)
        else {
            continue;
        };
        let entity = match entity_field.unwrap_defaulted() {
            Value::AssetRef(g) => *g,
            _ => continue,
        };
        let Ok(mut entity_stu) = data.stu_of(entity) else {
            continue;
        };
        let Some(def) = entity_stu.instance(ENTITY_DEFINITION) else {
            continue;
        };
        let map_field = match entity_stu.typed_field(def, COMPONENT_MAP_FIELD, reg) {
            Some(Value::Map(entries)) => entries,
            _ => continue,
        };
        sampled += 1;
        entries_total += map_field.len();
        let mut health = false;
        for (key, value) in &map_field {
            if *key == classes::HEALTH_COMPONENT as u64 {
                health = true;
            }
            if let Some(index) = value {
                assert!(
                    *index < entity_stu.instances.len(),
                    "component map value {index} is outside the instance table"
                );
                if entity_stu.instances[*index].hash as u64 == *key {
                    key_is_class += 1;
                }
            }
        }
        if health {
            with_health += 1;
        }
    }
    eprintln!(
        "component maps: {sampled} hero entities sampled, {with_health} carry a \
         STUHealthComponent (0x{:08X}) key; {entries_total} entries total, \
         {key_is_class} name their instance's class",
        classes::HEALTH_COMPONENT
    );
    assert!(sampled > 0, "no hero entity had a component map");
    assert!(
        with_health > 0,
        "no sampled hero entity's map carries the health component key"
    );

    // -----------------------------------------------------------------------
    // The hero-settings asset: typed layer vs the hand-written reader
    // -----------------------------------------------------------------------
    let settings_read = settings::read_settings_asset(&data, HERO_SETTINGS_ASSET)
        .expect("hero settings asset parses");
    let mut settings_stu = data
        .stu_of(HERO_SETTINGS_ASSET)
        .expect("hero settings asset parses");
    let float_instances = settings_stu.instances_of(classes::HERO_SETTING_FLOAT);
    assert_eq!(
        settings_read.len(),
        float_instances.len(),
        "the raw reader and the instance table disagree on the float-setting count"
    );

    let mut bounds_defaulted = 0usize;
    let mut max_defaulted = 0usize;
    let mut max_authored = 0usize;
    let mut max_missing = 0usize;
    let mut defaulted_slots = 0usize;
    for (setting, instance) in settings_read.iter().zip(float_instances.iter()) {
        if setting.bounds_are_defaulted() {
            bounds_defaulted += 1;
        }
        // Presence must agree slot for slot: the raw reader's `explicit` is
        // "field in the payload"; the typed layer says `Missing` for exactly
        // the same condition, and `Defaulted` when the stored bytes equal the
        // class default.
        for (slot, field) in [
            classes::hero_setting_float::DEFAULT,
            classes::hero_setting_float::MIN,
            classes::hero_setting_float::MAX,
            classes::hero_setting_float::STEP,
        ]
        .into_iter()
        .enumerate()
        {
            match settings_stu
                .typed_field(*instance, field, reg)
                .expect("setting field declared")
            {
                Value::Missing => {
                    assert!(
                        !setting.explicit[slot],
                        "instance {instance} field {field:#010x}: typed Missing but raw reader saw it"
                    );
                    if slot == 2 {
                        max_missing += 1;
                    }
                }
                Value::Prim(..) => {
                    assert!(
                        setting.explicit[slot],
                        "instance {instance} field {field:#010x}: typed present but raw reader did not"
                    );
                    if slot == 2 {
                        max_authored += 1;
                    }
                }
                Value::Defaulted(_) => {
                    assert!(
                        setting.explicit[slot],
                        "instance {instance} field {field:#010x}: typed Defaulted but raw reader did not see it"
                    );
                    defaulted_slots += 1;
                    if slot == 2 {
                        max_defaulted += 1;
                    }
                }
                other => panic!("instance {instance} field {field:#010x}: {other:?}"),
            }
        }
    }
    eprintln!(
        "hero settings 0x{HERO_SETTINGS_ASSET:016x}: {} float settings; of these \
         {bounds_defaulted} have both bounds at their class defaults (raw reading); \
         m_max: {max_authored} authored, {max_defaulted} present at the 1.0f class \
         default, {max_missing} absent",
        settings_read.len()
    );
    if defaulted_slots == 0 {
        eprintln!(
            "  note: no setting stores a field at its class default (the data \
             pipeline drops default-equal values); the Defaulted decoding is \
             exercised synthetically in ow-stu's types tests"
        );
    }
    assert!(
        bounds_defaulted > 0 && bounds_defaulted < settings_read.len(),
        "the defaulted-bounds classification is degenerate ({bounds_defaulted}/{})",
        settings_read.len()
    );
    assert_eq!(
        max_authored + max_defaulted + max_missing,
        settings_read.len(),
        "m_max accounting does not cover every setting"
    );
    assert!(max_authored > 0, "no setting authors m_max");
    assert!(max_missing > 0, "no setting omits m_max");

    // -----------------------------------------------------------------------
    // Weapon graphs: volley motions, typed path vs raw reader
    // -----------------------------------------------------------------------
    let heroes_for_weapons = data.guids_of_type(guid::types::HERO);
    let mut volleys_checked = 0usize;
    let mut motions_checked = 0usize;
    let mut fields_checked = 0usize;
    let mut graphs_checked = 0usize;
    let mut graphs_with_volleys = 0usize;
    let mut typed_only_motions = 0usize;
    let mut order_differences = 0usize;
    'graphs: for g in heroes_for_weapons
        .iter()
        .filter(|g| (1..=6).contains(&guid::index_of(**g)))
    {
        let Ok(mut h) = data.stu_of(*g) else { continue };
        let Some(instance) = h.instance(classes::HERO) else {
            continue;
        };
        let Some(entity_field) = h.typed_field(instance, classes::hero::GAMEPLAY_ENTITY, reg)
        else {
            continue;
        };
        let Value::AssetRef(entity) = entity_field.unwrap_defaulted() else {
            continue;
        };
        let entity = *entity;
        let Ok(weapons) = ow_tank::weapons::weapons_of(&data, entity) else {
            continue;
        };
        for weapon in &weapons {
            let Some(graph) = weapon.graph else { continue };
            let Ok(mut stu) = data.stu_of(graph) else {
                continue;
            };
            graphs_checked += 1;
            let raw_volleys = volley::volleys_of(&stu);
            if raw_volleys.is_empty() {
                continue;
            }
            graphs_with_volleys += 1;
            for v in &raw_volleys {
                if volleys_checked >= VOLLEY_CAP {
                    break 'graphs;
                }
                let motions = stu
                    .typed_field(v.state, classes::volley::PROJECTILE_MOTIONS, reg)
                    .expect("m_projectileMotions declared");
                let typed_indices: Vec<usize> = match motions.unwrap_defaulted() {
                    Value::Array(items) => items
                        .iter()
                        .filter_map(|item| match item.unwrap_defaulted() {
                            Value::Instance(i) => Some(*i),
                            Value::Null => None,
                            other => panic!("motion element {other:?}"),
                        })
                        .collect(),
                    Value::Null | Value::Missing => Vec::new(),
                    other => panic!("m_projectileMotions: {other:?}"),
                };
                let raw_indices: Vec<usize> = v.motions.iter().map(|m| m.instance).collect();
                // The raw reader resolves children through the instance
                // table's embedder links and matches the exact
                // STUStatescriptWeaponProjectileMotion hash (0x487AE227).
                // The typed array reads the stored indices directly, so it
                // sees motions the raw path cannot: instances with no
                // embedder back-link (top-level rather than embedded) and
                // subclass motions — every typed-only element observed on
                // this install is 0xE1A5B579, a subclass with no embedder.
                // Every raw motion must appear in the typed array; every
                // typed-only element must be motion-shaped.
                for raw in &raw_indices {
                    assert!(
                        typed_indices.contains(raw),
                        "graph 0x{graph:016x} volley {}: raw motion #{raw} is missing from \
                         the typed array {typed_indices:?}",
                        v.state
                    );
                }
                for extra in typed_indices.iter().filter(|i| !raw_indices.contains(i)) {
                    let class = stu.instances[*extra].hash;
                    assert!(
                        reg.field(class, classes::projectile_motion::A_30A41C77)
                            .is_some(),
                        "graph 0x{graph:016x} volley {}: typed-only element #{extra} \
                         (0x{class:08X}) is not motion-shaped",
                        v.state
                    );
                    let info = stu.instances[*extra];
                    eprintln!(
                        "  typed-only motion: graph 0x{graph:016x} volley {} element \
                         #{extra} class 0x{class:08X} embedder {}",
                        v.state,
                        if info.embedder_index >= 0 {
                            format!(
                                "#{} field {:#010x}",
                                info.embedder_index, info.embedder_field
                            )
                        } else {
                            "none".to_string()
                        }
                    );
                    typed_only_motions += 1;
                }
                if typed_indices.len() == raw_indices.len() && typed_indices != raw_indices {
                    order_differences += 1;
                }
                for m in &v.motions {
                    for field in [
                        classes::projectile_motion::A_30A41C77,
                        classes::projectile_motion::A_468EC3BE,
                    ] {
                        let typed = stu.typed_field(m.instance, field, reg).expect("declared");
                        let raw = volley::children_with_class(&stu, m.instance)
                            .into_iter()
                            .find(|(h, _, _)| *h == field);
                        fields_checked += 1;
                        match (typed.unwrap_defaulted(), raw) {
                            (Value::Instance(t), Some((_, class, r))) => {
                                assert_eq!(
                                    *t, r,
                                    "motion {} field {field:#010x}: typed index {t}, raw index {r}",
                                    m.instance
                                );
                                assert_eq!(
                                    stu.instances[*t].hash, class,
                                    "motion {} field {field:#010x}: class disagrees",
                                    m.instance
                                );
                            }
                            (Value::Null | Value::Missing, None) => {}
                            (t, r) => panic!(
                                "motion {} field {field:#010x}: typed {t:?} but raw child {r:?}",
                                m.instance
                            ),
                        }
                    }
                }
                volleys_checked += 1;
                motions_checked += v.motions.len();
            }
        }
    }
    eprintln!(
        "volley motions: {volleys_checked} volleys across {graphs_with_volleys} of \
         {graphs_checked} weapon graphs; {motions_checked} raw-visible motions (all \
         present in the typed arrays), {fields_checked} motion config-var fields \
         cross-checked; {typed_only_motions} typed-only motions (no embedder \
         back-link or a subclass of STUStatescriptWeaponProjectileMotion), \
         {order_differences} order differences"
    );
    assert!(volleys_checked > 0, "no volley state found in any graph");
    assert!(
        motions_checked + typed_only_motions > 0,
        "no volley had projectile motions at all; the typed array read is unexercised"
    );
    if motions_checked == 0 {
        eprintln!(
            "  note: on this install every motion in the sampled graphs is invisible \
             to the raw reader (no embedder back-link, or a motion subclass); the raw \
             comparison itself had nothing to run on"
        );
    }

    // -----------------------------------------------------------------------
    // Version-1 assets
    // -----------------------------------------------------------------------
    let mut v1_guids: Vec<u64> = Vec::new();
    for t in ow_tank::assoc::STU_V1_TYPES {
        v1_guids.extend(data.guids_of_type(*t));
    }
    if v1_guids.is_empty() {
        eprintln!("version 1: no asset of the v1 buckets is in the manifests; scan skipped");
        return;
    }
    let total = v1_guids.len();
    let stride = (total / V1_SCAN_CAP).max(1);
    let mut scanned = 0usize;
    let mut read = 0usize;
    let mut stud_headers = 0usize;
    let mut v1_parsed = 0usize;
    let mut v1_instances = 0usize;
    let mut typed_decoded = 0usize;
    let mut typed_unreadable = 0usize;
    let mut typed_assets = 0usize;
    let mut first: Option<String> = None;
    let mut first_error: Option<String> = None;
    for guid_v in v1_guids.iter().step_by(stride).take(V1_SCAN_CAP) {
        scanned += 1;
        let Ok(bytes) = data.bytes_of(*guid_v) else {
            continue;
        };
        read += 1;
        let has_magic = bytes.starts_with(&ow_stu::MAGIC_V1);
        if has_magic {
            stud_headers += 1;
        }
        match ow_stu::Stu::parse(bytes) {
            Ok(mut stu) => {
                if stu.is_v1() {
                    v1_parsed += 1;
                    v1_instances += stu.instances.len();
                    if first.is_none() {
                        first = Some(format!(
                            "0x{guid_v:016x}: {} instances",
                            stu.instances.len()
                        ));
                    }
                    // Decode a bounded slice with the registry.
                    if typed_assets < 3 {
                        typed_assets += 1;
                        for instance in 0..stu.instances.len().min(25) {
                            for field in stu.typed_fields(instance, reg) {
                                match field.value {
                                    Value::Unreadable => typed_unreadable += 1,
                                    Value::Raw(_) | Value::Missing => {}
                                    _ => typed_decoded += 1,
                                }
                            }
                        }
                    }
                }
                if has_magic {
                    assert!(
                        stu.is_v1(),
                        "0x{guid_v:016x}: starts with STUD but did not parse as version 1"
                    );
                }
            }
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(format!("0x{guid_v:016x}: {e}"));
                }
                if has_magic {
                    panic!("0x{guid_v:016x}: STUD header but parse failed: {e}");
                }
            }
        }
    }
    eprintln!(
        "version 1: scanned {scanned}/{total} (stride {stride}), {read} readable, \
         {stud_headers} with a STUD header, {v1_parsed} parsed as version 1 \
         ({v1_instances} instances)"
    );
    if let Some(line) = &first {
        eprintln!("  first: {line}");
    }
    if let Some(line) = &first_error {
        eprintln!("  first parse error: {line}");
    }
    eprintln!(
        "  typed decode on {typed_assets} asset(s): {typed_decoded} values, \
         {typed_unreadable} unreadable"
    );
    if stud_headers > 0 {
        assert!(v1_parsed > 0, "STUD header but no version-1 parse");
        assert!(
            typed_decoded > 0,
            "no typed field decoded from a version-1 asset"
        );
    }
}
