use ahjo_missions_contract::{Mission, MissionChange, commands::AddedCard};

#[derive(Clone, Copy)]
pub(crate) enum CreatedCard {
    InputCard,
    Formation,
    Gate,
    End,
    Limit,
}

impl CreatedCard {
    pub(crate) fn from_change(change: &MissionChange) -> Option<Self> {
        Some(match change {
            MissionChange::CreateInputCard { .. } => Self::InputCard,
            MissionChange::CreateFormation { .. } => Self::Formation,
            MissionChange::CreateGate { .. } => Self::Gate,
            MissionChange::CreateEnd { .. } => Self::End,
            MissionChange::CreateLimit { .. } => Self::Limit,
            _ => return None,
        })
    }

    pub(crate) fn added(self, before: &Mission, after: &Mission) -> Option<AddedCard> {
        fn first<'a, T>(before: &[T], after: &'a [T], id: impl Fn(&T) -> &str) -> Option<&'a T> {
            after
                .iter()
                .find(|item| !before.iter().any(|old| id(old) == id(item)))
        }

        match self {
            Self::InputCard => first(&before.input_cards, &after.input_cards, |n| &n.id)
                .cloned()
                .map(AddedCard::InputCard),
            Self::Formation => first(&before.formations, &after.formations, |n| &n.id)
                .cloned()
                .map(AddedCard::Formation),
            Self::Gate => first(&before.gates, &after.gates, |n| &n.id)
                .cloned()
                .map(AddedCard::Gate),
            Self::End => first(&before.ends, &after.ends, |n| &n.id)
                .cloned()
                .map(AddedCard::End),
            Self::Limit => first(&before.limits, &after.limits, |n| &n.id)
                .cloned()
                .map(AddedCard::Limit),
        }
    }
}
