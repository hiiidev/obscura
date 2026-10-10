//! Native initialization happens on the first context request, once per
//! process, and concurrent first requests share it. Kept apart from
//! `driver_tests`, whose ignored cases form the pinned native driver inventory.
//! nextest runs every test in a fresh process, which these assertions rely on.
use crate::{
    api::{Attributes, CanvasContext},
    egl::{native_initializations, NativeInitializations},
    selection::Mode,
};
use std::sync::{Arc, Barrier};

fn driver_mode() -> Mode {
    let mode = std::env::var("OBSCURA_WEBGL_BACKEND")
        .expect("select hardware or software explicitly for real-driver tests");
    assert!(matches!(mode.as_str(), "hardware" | "software"));
    Mode::parse(&mode).unwrap()
}

#[test]
fn policy_and_validation_alone_never_initialize_native_graphics() {
    // Mode parsing, attribute defaults and size validation are host-only.
    for value in ["auto", "hardware", "software", "invalid"] {
        let _ = Mode::parse(value);
    }
    let _ = Attributes::default();
    assert!(crate::egl::backing_size(0, 0).is_ok());
    assert!(crate::egl::backing_size(u32::MAX, u32::MAX).is_err());
    // Requests rejected before a display is needed load nothing either.
    for mode in [Mode::Auto, Mode::Hardware, Mode::Software] {
        assert!(CanvasContext::create(1, 40_000, 40_000, Attributes::default(), mode).is_err());
        assert!(CanvasContext::create(3, 1, 1, Attributes::default(), mode).is_err());
    }
    assert_eq!(native_initializations(), NativeInitializations::default());
}

#[test]
#[ignore = "real-driver concurrent first context requests share one library load and one display"]
fn real_concurrent_first_contexts_share_one_initialization() {
    let mode = driver_mode();
    assert_eq!(native_initializations(), NativeInitializations::default());
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|index| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                // Contexts belong to their creating thread; create and drop here.
                let version = if index % 2 == 0 { 1 } else { 2 };
                let context = CanvasContext::create(version, 4, 4, Attributes::default(), mode)
                    .map_err(|e| (e.reason, e.attempts.into_iter().map(|a| a.reason).collect::<Vec<_>>()))
                    .unwrap();
                (context.diagnostics.backend, context.diagnostics.renderer.clone())
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]), "{results:?}");
    let once = NativeInitializations { library_loads: 1, display_initializations: 1 };
    assert_eq!(native_initializations(), once);
    for version in [1, 2, 1] {
        let context = CanvasContext::create(version, 2, 2, Attributes::default(), mode).unwrap();
        assert_eq!(context.diagnostics.backend, results[0].0);
    }
    assert_eq!(native_initializations(), once, "later contexts reuse the process display");
}
