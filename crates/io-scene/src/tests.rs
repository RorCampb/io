use super::*;
#[test]
fn rig_contract_checks_numeric_intervals_and_named_bindings() {
    let mut rigs = CameraRigs::default();
    assert!(rigs.validate(&[]).is_ok());
    rigs.exterior.zoom = f32::NAN;
    assert!(rigs.validate(&[]).is_err());
    rigs.exterior = CameraRig::default();
    rigs.exterior.min_zoom = 4.;
    assert!(rigs.validate(&[]).is_err());
    rigs.exterior = CameraRig::default();
    rigs.interiors.insert("Room".into(), CameraRig::default());
    assert!(rigs.validate(&[]).is_err());
    assert!(rigs.validate(&["Room".into()]).is_ok());
}
