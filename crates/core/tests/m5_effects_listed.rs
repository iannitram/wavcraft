//! M5.1: a release build lists none of the effects v1 rules out (docs/v1/v1.md, "Deliberately
//! not in v1": DeHummer and DeReverb), and the registry indexes stay as they are.

use wavcraft_core::effects;

#[test]
fn a_release_build_lists_no_dehummer_or_dereverb() {
    let release: Vec<&str> = effects::listed(true).map(|(_, e)| e.name()).collect();
    assert!(release.contains(&"Noise Reduction"), "{release:?}");
    assert!(!release.contains(&"DeHummer"), "{release:?}");
    assert!(!release.contains(&"DeReverb"), "{release:?}");
    for id in effects::OUT_OF_V1 {
        assert!(effects::find(id).is_some(), "{id} is still registered (its code stays)");
    }

    let debug: Vec<(usize, &str)> = effects::listed(false).map(|(i, e)| (i, e.id())).collect();
    assert_eq!(debug.len(), effects::all().len());
    for (i, e) in effects::listed(true).chain(effects::listed(false)) {
        assert_eq!(effects::all()[i].id(), e.id(), "index {i} is the effect's place in the registry");
    }
}
