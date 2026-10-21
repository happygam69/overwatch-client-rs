//! Invariants of the hero's draw list, against the real install.
//!
//! These are the checks that would have caught the bug that removed the hero's
//! head: a submesh's draw wrote **absolute** indices into the shared index
//! buffer *and* set `base_vertex`, so the offset was counted twice and every
//! submesh after the first read the wrong vertices — and the last ones read past
//! the buffer. The scene looked plausible from a distance, which is exactly why
//! this is a test rather than an eyeball.
//!
//! Skips when no install is present.

#[test]
fn every_hero_draw_reads_vertices_that_exist() {
    let mut args: Vec<String> = vec!["--hero".into(), "3".into(), "--hero-only".into()];
    let scene = match ow_scene::build(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no install or no hero 3 ({e}); skipping");
            return;
        }
    };
    args.clear();
    assert!(!scene.draws.is_empty(), "the hero should produce draws");
    let vertex_count = scene.vertices.len() as i64;
    for (i, draw) in scene.draws.iter().enumerate() {
        assert!(draw.index_count > 0, "draw {i} has no indices");
        let start = draw.first_index as usize;
        let end = start + draw.index_count as usize;
        assert!(
            end <= scene.indices.len(),
            "draw {i} reads past the index buffer"
        );
        // The vertex a draw actually fetches: index relative to the model plus
        // `base_vertex`. Out of range means the offset convention is wrong.
        let mut touched = (i64::MAX, i64::MIN);
        for index in &scene.indices[start..end] {
            let vertex = *index as i64 + draw.base_vertex as i64;
            assert!(
                vertex >= 0 && vertex < vertex_count,
                "draw {i} fetches vertex {vertex} of {vertex_count}: the offset is counted twice"
            );
            touched.0 = touched.0.min(vertex);
            touched.1 = touched.1.max(vertex);
        }
        // And the range it touches must have real extent: a collapsed submesh is
        // a submesh that vanished (that is what the head did).
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for vertex in touched.0..=touched.1 {
            let p = scene.vertices[vertex as usize].position;
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
        assert!(
            extent > 0.001,
            "draw {i} (model 0x{:016x}) has zero extent: it would not be visible",
            draw.model
        );
    }
    eprintln!(
        "{} draws, {} vertices, {} indices: all in range, none collapsed",
        scene.draws.len(),
        scene.vertices.len(),
        scene.indices.len()
    );
}
