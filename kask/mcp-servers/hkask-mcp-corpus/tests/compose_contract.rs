use hkask_services_compose::cosine_distance;
use proptest::prelude::*;

proptest! {
    #[test]
    fn cosine_distance_is_symmetric(
        a in prop::collection::vec(prop::num::f32::NORMAL, 1..10),
        b in prop::collection::vec(prop::num::f32::NORMAL, 1..10),
    ) {
        let len = a.len().min(b.len());
        let a = &a[..len];
        let b = &b[..len];
        prop_assert_eq!(cosine_distance(a, b), cosine_distance(b, a));
    }

    #[test]
    fn cosine_distance_identity_zero(
        a in prop::collection::vec(prop::num::f32::NORMAL, 1..10),
    ) {
        let magnitude: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        prop_assume!(magnitude > 0.001);
        let d = cosine_distance(&a, &a);
        prop_assert!(d.abs() < 0.0001, "distance to self should be ~0.0, got {}", d);
    }

    #[test]
    fn cosine_distance_range(
        a in prop::collection::vec(prop::num::f32::NORMAL, 1..10),
        b in prop::collection::vec(prop::num::f32::NORMAL, 1..10),
    ) {
        let len = a.len().min(b.len());
        let a = &a[..len];
        let b = &b[..len];
        let d = cosine_distance(a, b);
        prop_assert!((0.0..=2.0).contains(&d), "distance {} out of [0,2]", d);
    }
}

#[test]
fn cosine_distance_empty() {
    assert_eq!(cosine_distance(&[], &[]), 2.0);
    assert_eq!(cosine_distance(&[], &[1.0]), 2.0);
    assert_eq!(cosine_distance(&[1.0], &[1.0, 2.0]), 2.0);
    assert_eq!(cosine_distance(&[0.0, 0.0], &[1.0, 2.0]), 2.0);
}

#[test]
fn cosine_distance_opposite() {
    let d = cosine_distance(&[1.0, 0.0], &[-1.0, 0.0]);
    assert!((d - 2.0).abs() < 0.0001, "got {}", d);
}

#[test]
fn cosine_distance_identical() {
    let d = cosine_distance(&[3.0, 4.0], &[3.0, 4.0]);
    assert!(d.abs() < 0.0001, "got {}", d);
}
