//! Checks the [`parvi_mission_model::EXAMPLES`] a new mission starts from.
use crate::Error;
use parvi_mission_model::{EXAMPLES, Example};

pub(crate) fn find(name: &str) -> Result<&'static Example, Error> {
    parvi_mission_model::example(name).ok_or_else(|| {
        let names: Vec<_> = EXAMPLES.iter().map(|e| e.name).collect();
        Error::new(
            "not_found",
            format!("no example {name:?}; the examples are {}", names.join(", ")),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Harnesses, RoleStore, validate};
    use parvi_mission_model::Mission;

    #[test]
    fn every_example_is_a_valid_mission() {
        for example in EXAMPLES {
            let mission = Mission::parse(example.definition)
                .unwrap_or_else(|e| panic!("{}: {e:?}", example.name));
            assert_eq!(mission.slug, example.name);
            assert_eq!(mission.title, example.title);
            let roles = RoleStore::new(std::env::temp_dir().join("parvi-no-roles"));
            let report = validate(&mission, &roles, &Harnesses::default());
            assert!(
                report.errors.is_empty(),
                "{}: {:?}",
                example.name,
                report.errors
            );
            assert!(
                report.warnings.is_empty(),
                "{}: {:?}",
                example.name,
                report.warnings
            );
        }
    }
}
