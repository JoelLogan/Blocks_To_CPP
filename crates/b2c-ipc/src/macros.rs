//! Macros shared by the DTO modules.

/// Defines a closed enum that crosses IPC as one of a fixed set of strings.
///
/// Each variant names its JSON text explicitly, so the serde names, the
/// [`VALUES`](crate::dto::Template::VALUES) list that the request schemas and the
/// isolation allowlist use, and the generated TypeScript union cannot drift apart.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $value:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS))]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $value)] $variant, )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The JSON text of every value, in declaration order.
            pub const VALUES: &'static [&'static str] = &[$($value),+];

            /// The JSON text of this value.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub(crate) use string_enum;
