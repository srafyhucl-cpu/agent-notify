macro_rules! define_string_id {
    ($name:ident) => {
        #[derive(
            Clone,
            Debug,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            serde::Deserialize,
            serde::Serialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, crate::DomainError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed != value {
                    return Err(crate::DomainError::InvalidIdentifier {
                        kind: stringify!($name),
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl TryFrom<String> for $name {
            type Error = crate::DomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

define_string_id!(AgentId);
define_string_id!(AgentSessionId);
define_string_id!(ChannelId);
define_string_id!(ChannelAccountId);
define_string_id!(ExternalMessageId);
define_string_id!(NotificationId);
define_string_id!(DeliveryId);
define_string_id!(InboundMessageId);
define_string_id!(RequestId);
