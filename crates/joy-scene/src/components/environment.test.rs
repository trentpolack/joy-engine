// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use joy_gameplay::EntityDefinition;

use super::*;

#[test]
fn levels_author_their_environment_and_bad_values_are_rejected() {
    let mut level = LevelData::new("Weather");
    assert_eq!(EnvironmentComponent::from_level(&level), Ok(None), "levels without one keep the game's defaults");

    let entity = EntityDefinition::from_json(&json!({
        "id": "weather",
        "components": {"environment": {
            "simulation": {"timeOfDay": 0.75, "wind": {"speed": 9}},
            "sky": {"clouds": {"coverage": 0.42}}
        }}
    }))
    .unwrap();
    level.entities.push(entity);
    let environment = EnvironmentComponent::from_level(&level).unwrap().expect("authored environment");
    assert_eq!(environment.simulation.time_of_day, 0.75);
    assert_eq!(environment.simulation.wind.speed, 9.0);
    assert_eq!(environment.simulation.latitude_degrees, EnvironmentOptions::default().latitude_degrees);
    assert_eq!(environment.sky.clouds.coverage, 0.42);

    let invalid = json!({"simulation": {"latitudeDegrees": 200}}).as_object().cloned().unwrap();
    assert!(EnvironmentComponent::read(&invalid).is_err());
    let unknown = json!({"sky": {"sparkle": 1}}).as_object().cloned().unwrap();
    assert!(EnvironmentComponent::read(&unknown).is_err());
}
