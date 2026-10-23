from __future__ import annotations

import bisect
import csv
import json
import pathlib
import sys

import build_translate as bt

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE = ROOT / "docs" / "function-names.csv"
MAP_PATH = pathlib.Path(r"D:\ow-dumps\build_2.25.0.0_to_2.25.0.1.json")
# The v2b docs frame answers to module_v2full.bin: it disagrees with
# module_after_diff_v2.bin on 4,621 of 10,582 shared nonzero pages
# (census 2026-10-21, see STATUS), so doc-frame windows come from here.
OLD_IMAGE = pathlib.Path(r"D:\ow-dumps\module_v2full.bin")
NEW_IMAGE = pathlib.Path(r"D:\ow-dumps\module_v21_clean.bin")

BASE = 0x7FF7C4D20000
EXEC_DELTA = 0x71820000
RANGE_DELTA = 0x17AEC0000
V2FULL = "module_v2full.bin"
V21 = "module_v21_clean.bin"


# (doc_address, frame, name, role, docref)
# frame: v2b = the standing capture's frame; exec = v2b - 0x71820000;
# range = v2b - 0x17AEC0000; v21 = already the current build.
SEED = [
    # ---------------- animation / clips / blend tree ----------------
    ("FUN_7ff7c68f1de0", "v2b", "MrnmHandler",
     "The MRNM chunk handler and its deserializer are one function: copies the header and copies the material values (materials.md) while building the runtime parent-indexed tables (0xA0/0xF8/0x18 strides, used-bone mask, roots at -1) (animation.md)",
     "animation.md:64; materials.md:203"),
    ("FUN_7ff7c6e7f1e0", "v2b", "ClipSectionAccessor",
     "The clip's section-indexed stream accessor: stream + (id+3)*0x10, the clip's own relocated section table",
     "animation.md:177; coverage-detail.md:26"),
    ("FUN_7ff7c6e85f40", "v2b", "ClipSection1Builder",
     "The section-1 builder: walks 0x18-stride entries, probes the six child descriptors, gate-blends the children",
     "animation.md:178"),
    ("0x7ff7c58ea110", "v2b", "AnimKind2Eval",
     "The kind-2 record evaluator: resolves the node, dispatches on the node kind, handles inline timeline/guid nodes, recurses otherwise",
     "animation.md:200"),
    ("FUN_7ff7c58e0ed0", "v2b", "CurveTimeWrapClamp",
     "The shared curve pipeline's time wrap/clamp stage",
     "animation.md:203"),
    ("FUN_7ff7c58dfbe0", "v2b", "CurveKeySampleSingle",
     "The shared curve pipeline's single keyframe search + lerp of record values",
     "animation.md:204"),
    ("FUN_7ff7c58df9b0", "v2b", "CurveKeySampleDual",
     "The shared curve pipeline's dual (two-source) sample stage feeding blend/replace",
     "animation.md:205"),
    ("FUN_7ff7c6a5f330", "v2b", "ClipNodeEvalPublic",
     "The clip's public node evaluator, reached from the 31-kind dispatcher",
     "coverage-detail.md:26"),
    ("FUN_7ff7c6ba3a70", "v2b", "ClipRuntimeConstruct",
     "The clip runtime record's constructor",
     "coverage-detail.md:26"),
    ("FUN_7ff7c6b67bc0", "v2b", "ClipRuntimeFactory",
     "The clip runtime's factory, reached through two data references (0x7ff7c80dd220 / 0x7ff7c815fae0)",
     "coverage-detail.md:26"),
    ("FUN_00d2a250", "v21", "AnimEventMaskDispatch",
     "Current-build animation event player helper: dispatches event masks 0x100/0x200/0x400/0x800 against the slot's event list (RVA 0xd2a250)",
     "animation.md:403"),
    ("FUN_00d3a020", "v21", "FpViewmodelEventPlayerB",
     "Current-build: the second event/state player the per-frame view-model updates call (RVA 0xd3a020)",
     "animation.md:401"),

    # ---------------- statescript ----------------
    ("FUN_7ff7c6eda1c0", "v2b", "StatescriptVarPoolConstruct",
     "Constructs the runtime variable pool (pool 0x68 -> holder 0x240 with the 16x0x20 slot table -> instance >=0x15B0)",
     "statescript.md:43"),
    ("FUN_7ff7c5771740", "v2b", "StatescriptRuntimeInit",
     "The script runtime instance's initializer; its only caller is the interface query wrapper",
     "statescript.md:95"),
    ("FUN_7ff7c6edc380", "v2b", "StatescriptApplyPass",
     "The apply pass: walks the instance's elements and applies each record's default",
     "statescript.md:100; coverage-detail.md:16"),
    ("FUN_7ff7c6edea10", "v2b", "StatescriptSlotResolve",
     "Resolves an element by its id in the runtime",
     "statescript.md:103; coverage-detail.md:16"),
    ("FUN_7ff7c57e6760", "v2b", "StatescriptElementRebind",
     "The element's rebind/lifecycle helper (frees and re-reads the element slot)",
     "statescript.md:108; coverage-detail.md:16"),
    ("FUN_7ff7c57679d0", "v2b", "StatescriptGateQuery",
     "The wrapper that queries the 302-user state-body interface and drives the manager's initializer",
     "statescript.md:130,242"),
    ("FUN_7ff7c5770230", "v2b", "StatescriptDriveWalk",
     "The per-frame script walk: slot 100 (+0x320) of the host class; walks the script-instance array and runs element slots 43/44 around a parameter push",
     "statescript.md:207; coverage-detail.md:16"),
    ("FUN_7ff7c6ec9070", "v2b", "StatescriptNodePush",
     "The parameter push: walks the element list and dispatches by node type index through the trampoline",
     "statescript.md:225,275; weapons.md:625"),
    ("FUN_7ff7c6ed4dc0", "v2b", "StatescriptNodeDispatch",
     "The node trampoline: entry = (mode==1 ? pair : pair+8)[node+0x1a8]",
     "statescript.md:285; coverage-detail.md:16"),
    ("FUN_7ff7c57e77b0", "v2b", "StatescriptSlot43Executor",
     "Element vtable slot 43 (+0x158): the executor step that sets the busy counter",
     "statescript.md:272"),
    ("FUN_7ff7c57e76f0", "v2b", "StatescriptSlot44Release",
     "Element vtable slot 44 (+0x160): releases the executor busy counter after the step",
     "statescript.md:274"),
    ("FUN_7ff7c5772080", "v2b", "StatescriptBind",
     "The host's bind path (slot +0x528): decodes the secure pointer, looks up the graph entry, counts the instances",
     "statescript.md:253"),
    ("FUN_7ff7c5771ee8", "v2b", "StatescriptElementAdopt",
     "The adopt helper: prepares the 0x40-byte element record",
     "statescript.md:259"),
    ("FUN_7ff7c74e10a0", "v2b", "StatescriptManagerConstruct",
     "The statescript manager's constructor; its tail writes +0x724 = (arg != 0) and the 0x80000 scratch",
     "statescript.md:372; coverage-detail.md:16"),
    ("FUN_7ff7c6f5c7a0", "v2b", "FieldRegister",
     "The shared field-register call: stores a field hash/offset pair into a type descriptor's record",
     "domino.md:654; statescript.md:402"),
    ("FUN_7ff7c6f7e310", "v2b", "StatescriptGraphMaterialize",
     "The graph materializer: walks the 0x10-byte node records {key, flags, kind, target} in order",
     "statescript.md:511; coverage-detail.md:16"),
    ("FUN_7ff7c6ff6880", "v2b", "StatescriptNodeMapBind",
     "Binds each materialized node into the key->node map at graph+0x1c8",
     "statescript.md:514; coverage-detail.md:16"),
    ("FUN_7ff7c6ff6ac0", "v2b", "StatescriptKeyVecAppend",
     "Appends the node key to the u32 vector at graph+0x1d0, swaps node+0xa8 and marks the rebind dirty byte at wrapper+0x2a0",
     "statescript.md:526; coverage-detail.md:16"),
    ("FUN_7ff7c6ff6ea0", "v2b", "StatescriptParamBlockApply",
     "Applies a record's 0x88-byte parameter block (target fields land at node+0x2b0/+0x2b4/+0x2b9/+0x2ba)",
     "statescript.md:512; coverage-detail.md:16"),
    ("FUN_7ff7c6fef270", "v2b", "StatescriptRefResolve",
     "Resolves a typed reference into the graph's tables: [graph+8][b1]+0x80+b2*8 -> object",
     "statescript.md:547; coverage-detail.md:16"),
    ("FUN_7ff7c6f72190", "v2b", "WorldQueryByClass",
     "The world query a resolved reference lands in (world, u16 class id, 0x9e)",
     "statescript.md:550; coverage-detail.md:16"),
    ("FUN_7ff7c63c4760", "v2b", "StatescriptCreateById",
     "The lazily-initialized band's id-keyed create-by-id service",
     "statescript.md:404"),

    # ---------------- config vars / input / camera ----------------
    ("FUN_7ff7c69975b0", "v2b", "ConfigVarRegister",
     "The config-var registration entry: takes the decoded names plus the static var-object address",
     "input-camera-audio.md:34"),
    ("FUN_7ff7c530cde0", "v2b", "MovementVarRegisterStub",
     "The one-shot stub registering player.GenerateRandomMovement",
     "input-camera-audio.md:41"),
    ("0x7ff7c533c4ee", "v2b", "InputVarRegister",
     "The registration stub for input.MouseScaleX/Y and input.ForceInputMode",
     "input-camera-audio.md:55"),
    ("0x7ff7c52c243e", "v2b", "CameraVarRegister",
     "The registration stub for the camera config-var block",
     "input-camera-audio.md:71"),
    ("FUN_7ff7c66aa450", "v2b", "TypeIndexByHash",
     "Maps the first-person component's class hash 0x9d28963f to type index 0x35",
     "animation.md:339"),

    # ---------------- maps / netcode / debug UI ----------------
    ("FUN_7ff7c6dabbb0", "v2b", "SceneMapLoadStep",
     "The scene's map-load step: validates the map GUID, binds the type-0x0790 map asset, drives the 0/1/3/5 state machine",
     "maps-streaming.md:24,174"),
    ("FUN_7ff7c5380590", "v2b", "LobbyServerConnectionUp",
     "The ClientLobbyMessageSystem::__LobbyServerConnectionUp handler: registry lookup, +0x94 state branch, message write, log",
     "netcode.md:10"),
    ("FUN_7ff7c52ac650", "v2b", "LobbyClientFind",
     "Looks the lobby client up by id in the registry; a null result asserts (non-returning)",
     "netcode.md:14"),
    ("FUN_7ff7c7b8d1f0", "v2b", "LobbyMessageWrite",
     "Hands the built 0x40-byte frame message to the lobby client's sink",
     "netcode.md:20"),
    ("FUN_7ff7c5933ec0", "v2b", "LobbyLog",
     "The shared lobby logger entry: (client, level, message, system-name string)",
     "netcode.md:21"),
    ("FUN_7ff7c6a17ed0", "v2b", "ImGuiWindowListPass",
     "The per-frame ImGui window-list/layout pass: hashes the id, applies clip rect, names windows and applies the attribute table",
     "ui-menu.md:8"),
    ("FUN_7ff7c6a021b0", "v2b", "ImGuiWindowBegin",
     "The ImGui Begin-shaped entry the pass calls with the cached ###NavWindowingList id",
     "ui-menu.md:30"),
    ("func_0x7ff64bf29130", "range", "MapLoaderConstruct",
     "Constructs the 0xa0-byte map loader object at the entry into load-state 1",
     "maps-streaming.md:58,164"),
    ("func_0x7ff64bf291f0", "range", "MapLoaderStart",
     "Starts the map loader object against the scene (loader, scene)",
     "maps-streaming.md:59,165"),
    ("func_0x7ff64be906b0", "range", "MapAssetLookup",
     "Looks up the assembled type-0x0790 map-header GUID, returning the asset object",
     "maps-streaming.md:52"),
    ("FUN_7ff64bef0340", "range", "MapLoadDiagnostic",
     "The map-load diagnostic path (called with 0x1d; one of the routes into load-state 3)",
     "maps-streaming.md:61"),
    ("func_0x7ff64bfc9940", "range", "MapPathBind",
     "Binds the flags&2 path object, called with 0x4000/0x8000 size arguments",
     "maps-streaming.md:84"),
    ("func_0x7ff64cb8cd70", "range", "MapNavHandlesRead",
     "Extracts the asset handles from an asset; stored as u16 at +0x31f8/+0x31fa",
     "maps-streaming.md:79"),

    # ---------------- materials chain ----------------
    ("FUN_7ff7c68e4560", "v2b", "MaterialAssetLookup",
     "The material-asset lookup (registry, keyA, keyB); falls into the no-return assert path on a miss",
     "coverage-detail.md:15"),
    ("0x7ff64b9e3320", "range", "MaterialLookupIndex",
     "The record-index lookup: answers the material table object with the record for a key (doc coin MaterialLookup_Index)",
     "materials.md:108; coverage-detail.md:15"),
    ("FUN_7ff7c68ec880", "v2b", "MaterialKeyBind",
     "The u64-array material binding: resolves keys -> indices clamped by the u64-value count",
     "coverage-detail.md:15"),
    ("FUN_7ff7c68f30a0", "v2b", "MaterialSlotBind",
     "The slot binder: sets the container's slot count and binds each material through the record's u16 at +4",
     "coverage-detail.md:15"),
    ("FUN_7ff7c68f5940", "v2b", "MaterialParamApply",
     "The material-parameter applier: resolves the material descriptor, scans the model's key/value records, stores the min/max times",
     "coverage-detail.md:15"),
    ("FUN_7ff7c68ee1c0", "v2b", "MaterialDescLookup",
     "The material descriptor lookup validated by a type query, with the (key,6) fallback",
     "coverage-detail.md:15"),
    ("FUN_7ff7c689dfa0", "v2b", "ScriptedFieldResolve",
     "The dynamic field resolution through ctx+0x98/0xa0/0xa8 gated by ctx+0xe2; else the raw dword at field+0x10",
     "coverage-detail.md:15"),
    ("FUN_7ff64b9e6640", "range", "ModelObjectDeserialize",
     "The per-model object deserializer (drives the per-chunk deserializers)",
     "materials.md:196"),
    ("FUN_7ff64ba31b80", "range", "MdlcMaterialRead",
     "The MDLC chunk deserializer: the material list (copies the header, memcpy's the material values)",
     "materials.md:86,202"),
    ("FUN_7ff64ba31de0", "range", "MrnmRead",
     "The MRNM chunk deserializer: the render mesh tables",
     "materials.md:203"),
    ("FUN_7ff64ba323a0", "range", "MsklTableBuild",
     "The mskl chunk deserializer: packs the 6-byte chain records into the table blob",
     "materials.md:99,204"),
    ("FUN_7ff64ba31d20", "range", "MoclRead",
     "The MOCL chunk deserializer: the occlusion/limits table",
     "materials.md:205"),
    ("FUN_7ff64ba318d0", "range", "HtlcFixupRead",
     "The HTLC chunk deserializer: memcpy + fixup with the pointer cached at runtime+0x18",
     "materials.md:207"),
    ("FUN_7ff64ba31cc0", "range", "MstuRead",
     "The MSTU chunk deserializer: structured data bound to runtime+0x80",
     "materials.md:208"),
    ("FUN_7ff64ba2de50", "range", "MaterialKeySource",
     "Returns the raw material keys from the entity's templates",
     "materials.md:119"),
    ("FUN_7ff64b7f23a0", "range", "MaterialBlendWeights",
     "The blend-weight consumer the material struct is passed on to",
     "materials.md:178"),

    # ---------------- domino physics ----------------
    ("0x7ff7c7a4cd70", "v2b", "DmWorldCreate",
     "The dmWorld factory: destroys the previous global world, scans the obfuscated handle table (0x3fff slots), allocates 0x4d0, calls the constructor, returns the handle",
     "domino.md:239"),
    ("FUN_7ff7c7a4cce0", "v2b", "DmWorldClearForces",
     "ClearForces (current build): decodes the handle, returns behind the +0x15c gate, walks the bodies zeroing force (0x130) and torque (0x140)",
     "domino.md:264"),
    ("0x7ff7c7a4d910", "v2b", "DmWorldReconfigure",
     "The per-handle reconfigure: decodes the handle, builds a context plus a settings copy, drives the big per-world re-init at world+0x1b0",
     "domino.md:267"),
    ("FUN_7ff7c7a7c460", "v2b", "BroadphaseUpdate",
     "The broadphase update run from the stepper at world+0x1b0",
     "domino.md:221"),
    ("FUN_7ff7c7a4b610", "v2b", "IslandBuild",
     "The island builder: walks contact/joint lists, stamps bodies with the island revision, collects islands (also fixes the world's runtime layout strides)",
     "domino.md:739"),
    ("FUN_7ff756261300", "exec", "IslandSolve",
     "The island solve: the velocity/position integration, damping clamp, quaternion renormalization and displacement clamp live here",
     "domino.md:742,762"),
    ("FUN_7ff756242d00", "exec", "ConstraintDispatch",
     "The per-constraint dispatcher (cases 1-9) inside the solve",
     "domino.md:743"),
    ("FUN_7ff75626b840", "exec", "ConstraintSolve1",
     "One of the per-type constraint solvers (dispatcher family)",
     "domino.md:744"),
    ("FUN_7ff75626bc30", "exec", "ConstraintSolve2",
     "One of the per-type constraint solvers (dispatcher family)",
     "domino.md:744"),
    ("FUN_7ff75626cbe0", "exec", "ConstraintSolve3",
     "One of the per-type constraint solvers (dispatcher family)",
     "domino.md:744"),
    ("FUN_7ff7562702a0", "exec", "RotationInertiaSolve",
     "A 4x4 rotation-inertia solver in the conditioned sweep (verified not the integrator)",
     "domino.md:684"),
    ("FUN_7ff7c7a4dbb0", "v2b", "DmHandleStepper",
     "The per-handle stepper wrapper: bounds-checks and decodes the handle, try-locks the busy flag, broadcasts dt, calls the stepper",
     "domino.md:342"),
    ("FUN_7ff7c7a4dcb0", "v2b", "DmHandleDriver",
     "The per-handle driver wrapper: the same try-lock shape calling the substep driver",
     "domino.md:343"),
    ("FUN_7ff7c6e0fc70", "v2b", "DmSystemTick",
     "The physics system tick: step context at [param+0x40], dt = 1/30 from the module table, iteration cap max(+0xef8, +0xefc)",
     "domino.md:339,360"),
    ("FUN_7ff7c7a68d80", "v2b", "DmStepEntry",
     "The step entry: repeats while the residual is below 0.5 and the iteration cap is not exhausted",
     "domino.md:340,355"),
    ("FUN_7ff7c7a68840", "v2b", "DmResidualQuery",
     "The residual query the step entry loops on (threshold 0.5 at DAT_7ff7c7d0c068)",
     "domino.md:356"),
    ("FUN_7ff7c7a68d30", "v2b", "DmCtxTimeGet",
     "The step context's time getter, used by the keeper's child catch-up",
     "domino.md:419,521"),
    ("FUN_7ff7c6d92691", "v2b", "DmFrameTickCall",
     "The frame-level caller that invokes the physics tick once, gated on +0xf08 bit 0x10 and mode +0x1b4 == 1",
     "domino.md:391"),
    ("FUN_7ff7c6e0fb50", "v2b", "DmTickKeeper",
     "The tick's keeper: zeroes the pending budget, clears the pending flag, recurses into children whose time is behind",
     "domino.md:399,418,515"),
    ("FUN_7ff7c6e028ec", "v2b", "DmTickConstruct",
     "The tick object's constructor: writes the pending/cap pair from one 64-bit argument and the flags byte from another",
     "domino.md:402,443"),
    ("FUN_7ff7c6e186d5", "v2b", "DmPendingPreload",
     "The startup catch-up: preloads pending = 150 (5 s at 1/30) when cap <= 0 and the startup flag is set",
     "domino.md:408"),
    ("FUN_7ff7c6da3da4", "v2b", "DmTickBuild",
     "The tick's builder (an owner method): allocates the 0xf10 object, calls the constructor and the bind, stores it at owner+0x128",
     "domino.md:439"),
    ("FUN_7ff7c6e08510", "v2b", "DmTickBind",
     "The tick bind: computes cap = ceil(ownerConfig.seconds*30), copies the config, stores the owner, installs the helper, clears the flags bits",
     "domino.md:481,493"),
    ("FUN_7ff7c7a5aee0", "v2b", "DmMassProperties",
     "The mass-properties computation: signed-tetrahedron integration over a mesh (volume, first/second moments; density, not a tick rate)",
     "domino.md:197"),
    ("FUN_7ff7c7a398b0", "v2b", "DmBodyHook",
     "dmBody's per-class hook: stores 0 at (dmBody+0x150)",
     "domino.md:588"),
    ("FUN_7ff7c6dac1b4", "v2b", "DmGameWorldsCreate",
     "The game-level world creator: calls dmWorld::Create twice and stores the two handles at +0x31f8/+0x31fa",
     "domino.md:242"),

    # ---------------- mover / character controller ----------------
    ("0x7ff7546622e0", "exec", "InitializeMoverComponent",
     "The mover component's constructor: installs its three interface vtables (+0/+8/+0x48)",
     "domino.md:620"),
    ("FUN_7ff64b7ea450", "range", "ComponentRouter",
     "The component router: maps 119 component ids to type hashes; the entity-component dispatch the pairing build used",
     "systems-plan.md:37,64"),
    ("FUN_7ff7c59ff690", "v2b", "MoverEntityAdvance",
     "The mover's advance (interface vtable slot 15): computes out = (a*X + pos)*b + off, reads the world handle and the flags; its character path is the collision sweep",
     "character-controller.md:576"),
    ("FUN_7ff7c5b14a84", "v2b", "MoverCollisionSweep",
     "The collision-sweep helper: builds a session and sweeps (i/3)*delta three times with a 1/60 request",
     "character-controller.md:522,598"),
    ("FUN_7ff7c7a45e40", "v2b", "DmSweepSessionCreate",
     "The collision sweep's session create",
     "domino.md:523; character-controller.md:599"),
    ("FUN_7ff7c7a46260", "v2b", "DmSweepStage",
     "The sweep stage the collision sweep loops through (with FUN_7ff7c7a45fe0)",
     "character-controller.md:524,600"),
    ("FUN_7ff7c5a0e610", "v2b", "MoverGroundProbe",
     "The ground probe the state machine calls conditionally",
     "character-controller.md:608"),
    ("FUN_7ff7c5a12c50", "v2b", "MoverStateMachine",
     "The per-frame character state machine: flags, timer, state id, the +0x1a79 state byte, the local vector and the ground probe",
     "character-controller.md:603"),
    ("FUN_7ff7c5c552e0", "v2b", "MoverStateTransA",
     "A mover state transition: preserves the local vector, clears a flag bit, runs the probe",
     "character-controller.md:611"),
    ("FUN_7ff7c5c54d80", "v2b", "MoverStateTransB",
     "A mover state transition (sibling of MoverStateTransA)",
     "character-controller.md:611"),
    ("FUN_7ff7c5c55a20", "v2b", "MoverStateTransC",
     "A mover state transition (sibling of MoverStateTransA)",
     "character-controller.md:611"),
    ("FUN_7ff7c59fd050", "v2b", "MoverWishUpdate",
     "The wish-vector update wrapper: reads +0x1140, converts through the frame-conversion helper, applies the difference",
     "character-controller.md:616"),
    ("FUN_7ff7c5a0ce70", "v2b", "MoverFrameConvert",
     "The frame conversion: inverts a matrix and transforms the passed vector into the stored space (not the input ramp)",
     "character-controller.md:621"),
    ("FUN_7ff7c59f7a70", "v2b", "MoverVisualSmooth",
     "The visual smoothing of the character's transform: lerp (dst-src)*k+src per 16-byte lane (the trace differ's settling)",
     "character-controller.md:648"),
    ("FUN_7ff7c59ff390", "v2b", "MoverSetup",
     "The mover's setup called from the attach cascade (its only caller read)",
     "character-controller.md:464"),
    ("FUN_7ff7c5540870", "v2b", "MoverClassRegister",
     "The mover class's registrar: allocates the definition, installs the factory table 0x7ff7c7d8dfd0 and registers through the type-registration core with size 0x1c00",
     "character-controller.md:416; coverage-detail.md:20"),
    ("FUN_7ff7c5540710", "v2b", "MoverFactoryAdapter",
     "The 18-byte factory adapter in the mover's factory table calling the shared init",
     "character-controller.md:432"),
    ("FUN_7ff7c5540750", "v2b", "MoverConcreteConstruct",
     "The mover's concrete constructor: installs the two concrete vtables (0x7ff7c7d8e000/0x7ff7c7d8e0b8) and initializes the tail fields",
     "character-controller.md:434"),
    ("FUN_7ff7c55407d0", "v2b", "MoverConcreteDestruct",
     "The mover's deleting destructor: releases the +0x1ba0 region, calls the base destructor, frees 0x1c00",
     "character-controller.md:438"),
    ("FUN_7ff7c69cc160", "v2b", "TypeRegistrationCore",
     "The type-registration core: allocates the per-flag registration tables",
     "character-controller.md:423; coverage-detail.md:20"),
    ("FUN_7ff7c69b7130", "v2b", "ComponentSharedInit",
     "The shared component-object init (321 callers, not per-class)",
     "character-controller.md:433"),

    # ---------------- debug panels ----------------
    ("FUN_7ff7c6a9c700", "v2b", "PsoDebugPanelRegister",
     "Decodes the encoded panel title, builds the panel object and registers the PSO GROUP COLLECTION overlay",
     "rendering.md:92"),
    ("FUN_7ff7c6a9d590", "v2b", "PsoDebugPanelDraw",
     "The draw callback of the PSO debug panel object",
     "rendering.md:98"),
    ("FUN_7ff7c6d55610", "v2b", "UiPanelRegister",
     "The debug UI panel registration entry called by the PSO stub",
     "rendering.md:100"),
]

# Table fixes: the existing row carries a name the docs disprove.
FIXES = [
    # domino.md:264 says this handle-level function is ClearForces, not the decode.
    {
        "program": V2FULL, "address": "0x7ff7c7a4cce0",
        "name": "DmWorldClearForces",
        "role": "ClearForces: decodes the handle, returns behind the +0x15c gate, and walks the bodies zeroing force (0x130) and torque (0x140)",
    },
]


def fix_rows() -> int:
    fixed = 0
    with TABLE.open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    for row in rows:
        for fix in FIXES:
            if fix["program"] in {p.strip() for p in row["program"].split(";")} \
                    and row["address"] == fix["address"]:
                row["name"] = fix["name"]
                row["role"] = fix["role"]
                fixed += 1
    if fixed:
        with TABLE.open("w", encoding="utf-8", newline="") as f:
            w = csv.DictWriter(f, fieldnames=["program", "address", "doc_address", "name", "role"])
            w.writeheader()
            w.writerows(rows)
    return fixed


def translate_doc_frame(
    old: bytes, new: bytes, off: int,
    new_sorted: list[int], new_ends: dict[int, int],
) -> tuple[int | None, str]:
    """The v21 begin and depth of a function whose doc-frame bytes sit at off.

    `old` is the docs-frame image, and a doc address is trusted as the
    function's begin ·· the frame image has no .pdata to gate with. The body's
    opening bytes are matched into the new image; a match counts only when it
    is unique (shared prologues have already misnamed candidates), and the
    result is snapped to the v21 body's own .pdata begin, with the depth the
    doc begin carries inside that body returned for eyeballing.
    """
    for width in (96, 64, 48):
        window = old[off : off + width]
        if window.count(0) == width:
            return None, "cold window in the docs frame"
        at = new.find(window)
        if at < 0:
            continue
        if new.find(window, at + 1) >= 0:
            return None, f"needle {width} not unique"
        i = bisect.bisect_right(new_sorted, at) - 1
        if i < 0:
            return None, "match below the first v21 begin"
        begin = new_sorted[i]
        if new_ends.get(begin, begin) <= at:
            return None, "match is past its v21 body end"
        if at - begin > 0x100:
            return None, f"match 0x{at - begin:x} deep inside a wider v21 body"
        return begin, at - begin
    return None, "not matched in 2.25.0.1"


def main() -> int:
    dry = "--dry" in sys.argv
    fixed = fix_rows()
    if fixed:
        print(f"applied {fixed} fix(es) to existing rows")

    old = OLD_IMAGE.read_bytes()
    new = NEW_IMAGE.read_bytes()
    new_fns = sorted(set(bt.functions(new)))
    new_ends = {b: e for b, e in new_fns}
    new_sorted = sorted(new_ends)
    print(f"new functions: {len(new_fns)}; the docs-frame image holds no .pdata,"
          " so a doc address is taken as its begin")

    with MAP_PATH.open(encoding="utf-8") as f:
        table = json.load(f)

    with TABLE.open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    names_used: set[tuple[str, str]] = set()
    addrs_used: dict[str, set[str]] = {}
    for row in rows:
        for prog in row["program"].split(";"):
            prog = prog.strip()
            names_used.add((prog, row["name"]))
            if prog != V21:
                continue
            addrs_used.setdefault(prog, set()).add(row["address"])

    added = skipped = 0
    for doc_addr, frame, name, role, docref in SEED:
        text = doc_addr.strip()
        stripped = False
        for form in ("func_0x", "func_", "FUN_0x", "FUN_"):
            if text.startswith(form):
                text = "0x" + text[len(form):]
                stripped = doc_addr != text
                break
        if not text.startswith("0x"):
            print(f"SKIP  {name:<24} {doc_addr:<20} unreadable address")
            skipped += 1
            continue
        addr = int(text, 16)
        if frame == "v21":
            # A stripped-program fun name (FUN_00d`` form) is an RVA: the docs
            # write FUN_00d2faa0 for the v21 address base+0xd2faa0, so an RVA
            # value under 2^32 is added to the base; a full v21 address is
            # used as is.
            new_addr = addr if addr > 0x10000000000 else BASE + addr
        else:
            if frame == "exec":
                v2b = addr + EXEC_DELTA
            elif frame == "range":
                v2b = addr + RANGE_DELTA
            else:
                v2b = addr
            if not (BASE <= v2b < BASE + len(old)):
                print(f"SKIP  {name:<24} {text:<20} outside the old image")
                skipped += 1
                continue
            off = v2b - BASE
            hint = table.get(str(off))
            if hint is not None:
                print(f"note  {name:<24} map json offers 0x{hint} (not trusted here)")
            begin, result = translate_doc_frame(old, new, off, new_sorted, new_ends)
            if begin is None:
                print(f"SKIP  {name:<24} {text:<20} {result}")
                skipped += 1
                continue
            depth = result
            new_addr = begin + BASE
        if (V21, name) in names_used:
            print(f"SKIP  {name:<24} name already in the table for {V21}")
            skipped += 1
            continue
        addr_str = f"0x{new_addr:x}"
        if addr_str in addrs_used.get(V21, set()):
            print(f"SKIP  {name:<24} {addr_str} already named in {V21}")
            skipped += 1
            continue
        names_used.add((V21, name))
        addrs_used.setdefault(V21, set()).add(addr_str)
        rows.append({
            "program": V21,
            "address": addr_str,
            "doc_address": doc_addr,
            "name": name,
            "role": role,
        })
        print(f"add   {name:<24} 0x{new_addr:x}   ({frame} {doc_addr}; {docref})"
              + ("" if not depth else f"; doc begin 0x{depth:x} into the v21 body"))
        added += 1

    if not dry and added:
        with TABLE.open("w", encoding="utf-8", newline="") as f:
            w = csv.DictWriter(f, fieldnames=["program", "address", "doc_address", "name", "role"])
            w.writeheader()
            w.writerows(rows)
    print(f"\n{added} rows added, {skipped} skipped, {fixed} fixed"
          + ("  (dry run, nothing written)" if dry else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
