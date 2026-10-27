//! The default scene is the resolved practice range, not the stand-in course.
//!
//! This is the launch gate: no arguments must resolve the install's practice
//! header, parse the variation's chunks and place the model-group entries. The
//! counts are expected values from the resolver test, not inputs.
//!
//! Skips when no install is present. Read-only.

#[test]
fn the_default_build_is_the_practice_range() {
    let scene = match ow_scene::build(&[]) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("no install ({e}); skipping");
            return;
        }
    };
    assert!(
        scene.sandbox_spawn.is_none(),
        "the default must not be the stand-in course"
    );
    assert_eq!(scene.models_loaded, 229, "distinct practice models");
    assert_eq!(scene.placements, 2449, "model-group entries placed");
    assert_eq!(scene.draws.len(), 872, "one draw per model/look/submesh");
    assert_eq!(
        scene.triangle_count(),
        1_216_581,
        "the practice range's instanced triangle count"
    );
    assert!(
        scene.textures.len() > 1,
        "the map's base-colour textures were decoded"
    );
    assert!(scene.eye.is_finite(), "the camera has a finite eye");
    assert!(!scene.draws.is_empty(), "the scene draws geometry");
}
