use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Version(i64);

impl Version {
    pub fn value(self) -> i64 {
        self.0
    }

    #[allow(dead_code)]
    pub(crate) const fn committed(value: i64) -> Self {
        Version(value)
    }
}

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Integral;

        impl serde::de::Visitor<'_> for Integral {
            type Value = i64;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an integral number")
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<i64, E> {
                Ok(value)
            }

            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<i64, E> {
                if value.fract() != 0.0
                    || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
                {
                    return Err(E::custom(format!(
                        "stored version {value} is not an integer"
                    )));
                }
                self.visit_i64(value as i64)
            }
        }

        deserializer.deserialize_any(Integral).map(Version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialization_accepts_integral_numbers_only() {
        let zero = mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::Int64(0)).unwrap();
        assert_eq!(zero.value(), 0);
        let positive = mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::Int64(2)).unwrap();
        assert_eq!(positive.value(), 2);
        let int32 = mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::Int32(2)).unwrap();
        assert_eq!(int32.value(), 2);
        let double = mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::Double(2.0)).unwrap();
        assert_eq!(double.value(), 2);
        assert!(mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::Double(2.5)).is_err());
        assert!(
            mongodb::bson::from_bson::<Version>(mongodb::bson::Bson::String("2".into())).is_err()
        );
    }
}
