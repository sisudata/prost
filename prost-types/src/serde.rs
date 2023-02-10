use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::de::{self, Visitor};
use serde::ser::{self, Error as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::datetime::{parse_timestamp, DateTime};
use crate::{value, Duration, Timestamp, Value};

#[cfg(feature = "std")]
impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&DateTime::from(self.clone()).to_string())
    }
}

struct TimestampVisitor;

#[cfg(feature = "std")]
impl<'de> Visitor<'de> for TimestampVisitor {
    type Value = Timestamp;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a valid RFC 3339 timestamp string")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        parse_timestamp(value)
            .ok_or_else(|| de::Error::invalid_value(de::Unexpected::Str(value), &self))
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Timestamp, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(TimestampVisitor)
    }
}

impl Serialize for Duration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct DurationVisitor;

#[cfg(feature = "std")]
impl<'de> Visitor<'de> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a valid duration string")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let value = match value.strip_suffix('s') {
            Some(value) => value,
            None => return Err(de::Error::custom(format!("invalid duration: {}", value))),
        };
        let seconds = value.parse::<f64>().map_err(de::Error::custom)?;

        if seconds.is_sign_negative() {
            let Duration { seconds, nanos } = std::time::Duration::from_secs_f64(-seconds)
                .try_into()
                .map_err(de::Error::custom)?;

            Ok(Duration {
                seconds: -seconds,
                nanos: -nanos,
            })
        } else {
            Ok(std::time::Duration::from_secs_f64(seconds)
                .try_into()
                .map_err(de::Error::custom)?)
        }
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DurationVisitor)
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // It's invalid to serialize a `Value` without a variant set, per [1].
        //
        // [1]: https://developers.google.com/protocol-buffers/docs/reference/google.protobuf#google.protobuf.Value
        let kind = self
            .kind
            .as_ref()
            .ok_or_else(|| S::Error::custom("invalid value: variant must be set"))?;
        match kind {
            value::Kind::NullValue(_) => serializer.serialize_unit(),
            value::Kind::NumberValue(value) => {
                // Per [1]:
                //
                // > Note that attempting to serialize NaN or Infinity results in error. (We can't
                // > serialize these as string "NaN" or "Infinity" values like we do for regular
                // > fields, because they would parse as string_value, not number_value).
                if !value.is_finite() {
                    return Err(S::Error::custom(
                        "number values must not be NaN or infinite",
                    ));
                }
                serializer.serialize_f64(*value)
            }
            value::Kind::StringValue(value) => serializer.serialize_str(value),
            value::Kind::BoolValue(value) => serializer.serialize_bool(*value),
            value::Kind::StructValue(crate::Struct { fields }) => fields.serialize(serializer),
            value::Kind::ListValue(crate::ListValue { values }) => values.serialize(serializer),
        }
    }
}

struct ValueVisitor;

#[cfg(feature = "std")]
impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a valid value")
    }

    #[inline]
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    #[inline]
    fn visit_i64<E>(self, value: i64) -> Result<Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    #[inline]
    fn visit_u64<E>(self, value: u64) -> Result<Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    #[inline]
    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    #[inline]
    fn visit_str<E>(self, value: &str) -> Result<Value, E>
    where
        E: de::Error,
    {
        self.visit_string(String::from(value))
    }

    #[inline]
    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    #[inline]
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::null())
    }

    #[inline]
    fn visit_some<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
    }

    #[inline]
    fn visit_unit<E>(self) -> Result<Value, E>
    where
        E: de::Error,
    {
        self.visit_none()
    }

    #[inline]
    fn visit_seq<V>(self, mut visitor: V) -> Result<Value, V::Error>
    where
        V: de::SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(visitor.size_hint().unwrap_or(0));

        while let Some(elem) = visitor.next_element()? {
            values.push(elem);
        }

        Ok(Value {
            kind: Some(value::Kind::ListValue(crate::ListValue { values })),
        })
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    fn visit_map<V>(self, mut visitor: V) -> Result<Value, V::Error>
    where
        V: de::MapAccess<'de>,
    {
        let mut fields = BTreeMap::new();

        while let Some((key, value)) = visitor.next_entry()? {
            fields.insert(key, value);
        }

        Ok(Value {
            kind: Some(value::Kind::StructValue(crate::Struct { fields })),
        })
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ValueVisitor)
    }
}

pub trait HasConstructor {
    fn new() -> Self;
}

pub struct MyType<'de, T: de::Visitor<'de> + HasConstructor>(<T as de::Visitor<'de>>::Value);

impl<'de, T> Deserialize<'de> for MyType<'de, T>
where
    T: de::Visitor<'de> + HasConstructor,
{
    fn deserialize<D>(deserializer: D) -> Result<MyType<'de, T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer
            .deserialize_any(T::new())
            .map(|x| MyType { 0: x })
    }
}

pub fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

pub mod empty {
    use super::*;

    struct EmptyVisitor;
    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for EmptyVisitor {
        type Value = ();

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid empty object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let tmp: Option<((), ())> = map.next_entry()?;
            if tmp.is_some() {
                Err(de::Error::custom("this is a message, not empty"))
            } else {
                Ok(())
            }
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<(), D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(EmptyVisitor)
    }

    pub fn serialize<S>(_: &(), serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use ser::SerializeMap;
        let map = serializer.serialize_map(Some(0))?;
        map.end()
    }
}

pub mod empty_opt {
    use super::*;

    struct EmptyVisitor;
    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for EmptyVisitor {
        type Value = Option<()>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid empty object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let tmp: Option<((), ())> = map.next_entry()?;
            if tmp.is_some() {
                Err(de::Error::custom("this is a message, not empty"))
            } else {
                Ok(Some(()))
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(()))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<()>, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(EmptyVisitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(opt: &Option<()>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use ser::SerializeMap;
        if opt.is_some() {
            let map = serializer.serialize_map(Some(0))?;
            map.end()
        } else {
            serializer.serialize_none()
        }
    }
}

pub mod vec {
    use super::*;

    struct VecVisitor<'de, T>
    where
        T: Deserialize<'de>,
    {
        _vec_type: &'de std::marker::PhantomData<T>,
    }

    #[cfg(feature = "std")]
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for VecVisitor<'de, T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid list")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(seq.size_hint().unwrap_or(0));
            loop {
                match seq.next_element()? {
                    Some(el) => res.push(el),
                    None => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T: 'de + Deserialize<'de>>(
        deserializer: D,
    ) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(VecVisitor::<'de, T> {
            _vec_type: &std::marker::PhantomData,
        })
    }
}

pub mod repeated {
    use super::*;

    struct VecVisitor<'de, T>
    where
        T: de::Visitor<'de> + HasConstructor,
    {
        _vec_type: &'de std::marker::PhantomData<T>,
    }

    #[cfg(feature = "std")]
    impl<'de, T> de::Visitor<'de> for VecVisitor<'de, T>
    where
        T: de::Visitor<'de> + HasConstructor,
    {
        type Value = Vec<<T as de::Visitor<'de>>::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid repeated field")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(seq.size_hint().unwrap_or(0));
            loop {
                let response: Option<MyType<'de, T>> = seq.next_element()?;
                match response {
                    Some(el) => res.push(el.0),
                    None => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T: 'de + de::Visitor<'de> + HasConstructor>(
        deserializer: D,
    ) -> Result<Vec<<T as de::Visitor<'de>>::Value>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(VecVisitor::<'de, T> {
            _vec_type: &std::marker::PhantomData,
        })
    }

    pub fn serialize<S, F>(
        value: &[<F as SerializeMethod>::Value],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: SerializeMethod,
    {
        use ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(value.len()))?;
        for e in value {
            seq.serialize_element(&MySeType::<F> { val: e })?;
        }
        seq.end()
    }
}

pub mod enum_serde {
    use super::*;

    pub struct EnumVisitor<'de, T>
    where
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        _type: &'de std::marker::PhantomData<T>,
    }

    impl<T> HasConstructor for EnumVisitor<'_, T>
    where
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        fn new() -> Self {
            Self {
                _type: &std::marker::PhantomData,
            }
        }
    }

    #[cfg(feature = "std")]
    impl<'de, T> de::Visitor<'de> for EnumVisitor<'de, T>
    where
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        type Value = i32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid string or integer representation of an enum")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match T::from_str(value) {
                Ok(en) => Ok(en.into()),
                Err(_) => Err(de::Error::invalid_value(de::Unexpected::Str(value), &self)),
            }
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match T::try_from(value as i32) {
                Ok(en) => Ok(en.into()),
                // There is a test in the conformance tests:
                // Required.Proto3.JsonInput.EnumFieldUnknownValue.Validator
                // That implies this should return the default value, so we
                // will. This also helps when parsing a oneof, since this means
                // we won't fail to deserialize when we have an out of bounds
                // enum value.
                Err(_) => Ok(T::default().into()),
            }
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_i64(value as i64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_i64(value as i64)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de
            + ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        deserializer.deserialize_any(EnumVisitor::<'de, T> {
            _type: &std::marker::PhantomData,
        })
    }

    pub fn serialize<S, T>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        match T::try_from(*value) {
            Err(_) => Err(ser::Error::custom("invalid enum value")),
            Ok(t) => serializer.serialize_str(&t.to_string()),
        }
    }

    pub struct EnumSerializer<T>
    where
        T: std::convert::TryFrom<i32> + ToString,
    {
        _type: std::marker::PhantomData<T>,
    }

    impl<T> SerializeMethod for EnumSerializer<T>
    where
        T: std::convert::TryFrom<i32> + ToString,
    {
        type Value = i32;

        fn serialize<S>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match T::try_from(*value) {
                Err(_) => Err(ser::Error::custom("invalid enum value")),
                Ok(t) => serializer.serialize_str(&t.to_string()),
            }
        }
    }
}

pub mod enum_opt {
    use super::*;

    struct EnumVisitor<'de, T>
    where
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        _type: &'de std::marker::PhantomData<T>,
    }

    #[cfg(feature = "std")]
    impl<'de, T> de::Visitor<'de> for EnumVisitor<'de, T>
    where
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        type Value = Option<i32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid string or integer representation of an enum")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match T::from_str(value) {
                Ok(en) => Ok(Some(en.into())),
                Err(_) => Err(de::Error::invalid_value(de::Unexpected::Str(value), &self)),
            }
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match T::try_from(value as i32) {
                Ok(en) => Ok(Some(en.into())),
                // There is a test in the conformance tests:
                // Required.Proto3.JsonInput.EnumFieldUnknownValue.Validator
                // That implies this should return the default value, so we
                // will. This also helps when parsing a oneof, since this means
                // we won't fail to deserialize when we have an out of bounds
                // enum value.
                Err(_) => Ok(Some(T::default().into())),
            }
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_i64(value as i64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_i64(value as i64)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<i32>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de
            + ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        deserializer.deserialize_any(EnumVisitor::<'de, T> {
            _type: &std::marker::PhantomData,
        })
    }

    pub fn serialize<S, T>(value: &Option<i32>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: ToString
            + std::str::FromStr
            + std::convert::Into<i32>
            + std::convert::TryFrom<i32>
            + Default,
    {
        match value {
            None => serializer.serialize_none(),
            Some(enum_int) => enum_serde::EnumSerializer::<T>::serialize(enum_int, serializer),
        }
    }
}

pub mod btree_map_custom_value {
    use super::*;

    struct MapVisitor<'de, T, V>
    where
        T: Deserialize<'de>,
        V: de::Visitor<'de> + HasConstructor,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de V>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, V> de::Visitor<'de> for MapVisitor<'de, T, V>
    where
        T: Deserialize<'de> + std::cmp::Eq + std::cmp::Ord,
        V: de::Visitor<'de> + HasConstructor,
    {
        type Value = BTreeMap<T, <V as de::Visitor<'de>>::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::new();
            loop {
                let response: Option<(T, MyType<'de, V>)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key, val.0);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, V>(
        deserializer: D,
    ) -> Result<BTreeMap<T, <V as de::Visitor<'de>>::Value>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + Deserialize<'de> + std::cmp::Eq + std::cmp::Ord,
        V: 'de + de::Visitor<'de> + HasConstructor,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, V> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, T, F>(
        value: &BTreeMap<T, <F as SerializeMethod>::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize + std::cmp::Eq + std::cmp::Ord,
        F: SerializeMethod,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&key, &MySeType::<F> { val: value })?;
        }
        map.end()
    }
}

pub mod map_custom_value {
    use super::*;

    struct MapVisitor<'de, T, V>
    where
        T: Deserialize<'de>,
        V: de::Visitor<'de> + HasConstructor,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de V>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, V> de::Visitor<'de> for MapVisitor<'de, T, V>
    where
        T: Deserialize<'de> + std::cmp::Eq + std::hash::Hash,
        V: de::Visitor<'de> + HasConstructor,
    {
        type Value = HashMap<T, <V as de::Visitor<'de>>::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(map.size_hint().unwrap_or(0));
            loop {
                let response: Option<(T, MyType<'de, V>)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key, val.0);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, V>(
        deserializer: D,
    ) -> Result<HashMap<T, <V as de::Visitor<'de>>::Value>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + Deserialize<'de> + std::cmp::Eq + std::hash::Hash,
        V: 'de + de::Visitor<'de> + HasConstructor,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, V> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, T, F>(
        value: &HashMap<T, <F as SerializeMethod>::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize + std::cmp::Eq + std::hash::Hash,
        F: SerializeMethod,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&key, &MySeType::<F> { val: value })?;
        }
        map.end()
    }
}

pub mod map_custom {
    use super::*;

    struct MapVisitor<'de, T, V>
    where
        T: de::Visitor<'de> + HasConstructor,
        V: Deserialize<'de>,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de V>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, V> de::Visitor<'de> for MapVisitor<'de, T, V>
    where
        T: de::Visitor<'de> + HasConstructor,
        V: Deserialize<'de>,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::hash::Hash,
    {
        type Value = HashMap<<T as de::Visitor<'de>>::Value, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(map.size_hint().unwrap_or(0));
            loop {
                let response: Option<(MyType<'de, T>, V)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key.0, val);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, V>(
        deserializer: D,
    ) -> Result<HashMap<<T as de::Visitor<'de>>::Value, V>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + de::Visitor<'de> + HasConstructor,
        V: 'de + Deserialize<'de>,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::hash::Hash,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, V> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, F, V>(
        value: &HashMap<<F as SerializeMethod>::Value, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: SerializeMethod,
        V: Serialize,
        <F as SerializeMethod>::Value: std::cmp::Eq + std::hash::Hash,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&MySeType::<F> { val: key }, &value)?;
        }
        map.end()
    }
}

pub mod map_custom_to_custom {
    use super::*;

    struct MapVisitor<'de, T, S>
    where
        T: de::Visitor<'de> + HasConstructor,
        S: de::Visitor<'de> + HasConstructor,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de S>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, S> de::Visitor<'de> for MapVisitor<'de, T, S>
    where
        T: de::Visitor<'de> + HasConstructor,
        S: de::Visitor<'de> + HasConstructor,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::hash::Hash,
    {
        type Value = HashMap<<T as de::Visitor<'de>>::Value, <S as de::Visitor<'de>>::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(map.size_hint().unwrap_or(0));
            loop {
                let response: Option<(MyType<'de, T>, MyType<'de, S>)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key.0, val.0);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, S>(
        deserializer: D,
    ) -> Result<HashMap<<T as de::Visitor<'de>>::Value, <S as de::Visitor<'de>>::Value>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + de::Visitor<'de> + HasConstructor,
        S: 'de + de::Visitor<'de> + HasConstructor,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::hash::Hash,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, S> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, F, G>(
        value: &HashMap<<F as SerializeMethod>::Value, <G as SerializeMethod>::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: SerializeMethod,
        G: SerializeMethod,
        <F as SerializeMethod>::Value: std::cmp::Eq + std::hash::Hash,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&MySeType::<F> { val: key }, &MySeType::<G> { val: value })?;
        }
        map.end()
    }
}

pub mod btree_map_custom {
    use super::*;

    struct MapVisitor<'de, T, V>
    where
        T: de::Visitor<'de> + HasConstructor,
        V: Deserialize<'de>,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de V>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, V> de::Visitor<'de> for MapVisitor<'de, T, V>
    where
        T: de::Visitor<'de> + HasConstructor,
        V: Deserialize<'de>,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        type Value = BTreeMap<<T as de::Visitor<'de>>::Value, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::new();
            loop {
                let response: Option<(MyType<'de, T>, V)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key.0, val);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, V>(
        deserializer: D,
    ) -> Result<BTreeMap<<T as de::Visitor<'de>>::Value, V>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + de::Visitor<'de> + HasConstructor,
        V: 'de + Deserialize<'de>,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, V> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, F, V>(
        value: &BTreeMap<<F as SerializeMethod>::Value, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: SerializeMethod,
        V: Serialize,
        <F as SerializeMethod>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&MySeType::<F> { val: key }, &value)?;
        }
        map.end()
    }
}

pub mod btree_map_custom_to_custom {
    use super::*;

    struct MapVisitor<'de, T, S>
    where
        T: de::Visitor<'de> + HasConstructor,
        S: de::Visitor<'de> + HasConstructor,
    {
        _map_type: fn() -> (
            std::marker::PhantomData<&'de T>,
            std::marker::PhantomData<&'de S>,
        ),
    }

    #[cfg(feature = "std")]
    impl<'de, T, S> de::Visitor<'de> for MapVisitor<'de, T, S>
    where
        T: de::Visitor<'de> + HasConstructor,
        S: de::Visitor<'de> + HasConstructor,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        type Value = BTreeMap<<T as de::Visitor<'de>>::Value, <S as de::Visitor<'de>>::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::new();
            loop {
                let response: Option<(MyType<'de, T>, MyType<'de, S>)> = map.next_entry()?;
                match response {
                    Some((key, val)) => {
                        res.insert(key.0, val.0);
                    }
                    _ => return Ok(res),
                }
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D, T, S>(
        deserializer: D,
    ) -> Result<BTreeMap<<T as de::Visitor<'de>>::Value, <S as de::Visitor<'de>>::Value>, D::Error>
    where
        D: Deserializer<'de>,
        T: 'de + de::Visitor<'de> + HasConstructor,
        S: 'de + de::Visitor<'de> + HasConstructor,
        <T as de::Visitor<'de>>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        deserializer.deserialize_any(MapVisitor::<'de, T, S> {
            _map_type: || (std::marker::PhantomData, std::marker::PhantomData),
        })
    }

    pub fn serialize<S, F, G>(
        value: &BTreeMap<<F as SerializeMethod>::Value, <G as SerializeMethod>::Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: SerializeMethod,
        G: SerializeMethod,
        <F as SerializeMethod>::Value: std::cmp::Eq + std::cmp::Ord,
    {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(&MySeType::<F> { val: key }, &MySeType::<G> { val: value })?;
        }
        map.end()
    }
}

pub trait SerializeMethod {
    type Value;
    fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer;
}

pub struct MySeType<'a, T>
where
    T: SerializeMethod,
{
    val: &'a <T as SerializeMethod>::Value,
}

impl<'a, T: SerializeMethod> Serialize for MySeType<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        T::serialize(self.val, serializer)
    }
}

pub mod map {
    use super::*;

    use std::collections::HashMap;

    struct MapVisitor<'de, K, V>
    where
        K: Deserialize<'de> + std::cmp::Eq + std::hash::Hash,
        V: Deserialize<'de>,
    {
        _key_type: &'de std::marker::PhantomData<K>,
        _value_type: &'de std::marker::PhantomData<V>,
    }

    #[cfg(feature = "std")]
    impl<'de, K: Deserialize<'de> + std::cmp::Eq + std::hash::Hash, V: Deserialize<'de>>
        de::Visitor<'de> for MapVisitor<'de, K, V>
    {
        type Value = HashMap<K, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::with_capacity(map.size_hint().unwrap_or(0));
            loop {
                match map.next_entry()? {
                    Some((k, v)) => {
                        res.insert(k, v);
                    }
                    None => return Ok(res),
                }
            }
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<
        'de,
        D,
        K: 'de + Deserialize<'de> + std::cmp::Eq + std::hash::Hash,
        V: 'de + Deserialize<'de>,
    >(
        deserializer: D,
    ) -> Result<HashMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(MapVisitor::<'de, K, V> {
            _key_type: &std::marker::PhantomData,
            _value_type: &std::marker::PhantomData,
        })
    }
}

pub mod btree_map {
    use super::*;

    use std::collections::BTreeMap;

    struct MapVisitor<'de, K, V>
    where
        K: Deserialize<'de> + std::cmp::Eq + std::cmp::Ord,
        V: Deserialize<'de>,
    {
        _key_type: &'de std::marker::PhantomData<K>,
        _value_type: &'de std::marker::PhantomData<V>,
    }

    #[cfg(feature = "std")]
    impl<'de, K: Deserialize<'de> + std::cmp::Eq + std::cmp::Ord, V: Deserialize<'de>>
        de::Visitor<'de> for MapVisitor<'de, K, V>
    {
        type Value = BTreeMap<K, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut res = Self::Value::new();
            loop {
                match map.next_entry()? {
                    Some((k, v)) => {
                        res.insert(k, v);
                    }
                    None => return Ok(res),
                }
            }
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<
        'de,
        D,
        K: 'de + Deserialize<'de> + std::cmp::Eq + std::cmp::Ord,
        V: 'de + Deserialize<'de>,
    >(
        deserializer: D,
    ) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(MapVisitor::<'de, K, V> {
            _key_type: &std::marker::PhantomData,
            _value_type: &std::marker::PhantomData,
        })
    }
}

pub mod string {
    use super::*;

    struct StringVisitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for StringVisitor {
        type Value = std::string::String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<std::string::String, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StringVisitor)
    }
}

pub mod string_opt {
    use super::*;

    struct StringVisitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for StringVisitor {
        type Value = Option<std::string::String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<std::string::String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StringVisitor)
    }
}

pub mod bool {
    use super::*;

    pub struct BoolVisitor;

    impl HasConstructor for BoolVisitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid boolean")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(bool::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(BoolVisitor)
    }
}

pub mod bool_map_key {
    use super::*;

    pub struct BoolVisitor;

    impl HasConstructor for BoolVisitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid boolean")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(de::Error::invalid_type(de::Unexpected::Str(value), &self)),
            }
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(BoolVisitor)
    }

    pub struct BoolKeySerializer;

    impl SerializeMethod for BoolKeySerializer {
        type Value = bool;
        #[cfg(feature = "std")]
        fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            if *value {
                serializer.serialize_str("true")
            } else {
                serializer.serialize_str("false")
            }
        }
    }
}

pub mod bool_opt {
    use super::*;

    struct BoolVisitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = Option<bool>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid boolean")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(BoolVisitor)
    }
}

pub mod i32 {
    use super::*;

    pub struct I32Visitor;

    impl HasConstructor for I32Visitor {
        fn new() -> I32Visitor {
            I32Visitor {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for I32Visitor {
        type Value = i32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid i32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i32::try_from(value).map_err(E::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value > i32::MAX as f64
                || value < i32::MIN as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(value as i32)
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i32::try_from(value).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<i32>().map_err(E::custom)
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(i32::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(I32Visitor)
    }
}

pub mod i32_opt {
    use super::*;

    struct I32Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for I32Visitor {
        type Value = Option<i32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid i32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value > i32::MAX as f64
                || value < i32::MIN as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(Some(value as i32))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<i32>().map(Some).map_err(E::custom)
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(I32Visitor)
    }
}

pub mod i64 {
    use super::*;

    pub struct I64Visitor;

    impl HasConstructor for I64Visitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for I64Visitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid i64")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as i64)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value > i64::MAX as f64
                || value < i64::MIN as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(value as i64)
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i64::try_from(value).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<i64>().map_err(E::custom)
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(i64::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(I64Visitor)
    }

    pub struct I64Serializer;

    impl SerializeMethod for I64Serializer {
        type Value = i64;
        #[cfg(feature = "std")]
        fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&value.to_string())
        }
    }
}

pub mod i64_opt {
    use super::*;

    struct I64Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for I64Visitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid i64")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as i64))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value > i64::MAX as f64
                || value < i64::MIN as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(Some(value as i64))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            i64::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<i64>().map(Some).map_err(E::custom)
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(I64Visitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(double) => i64::I64Serializer::serialize(double, serializer),
        }
    }
}

pub mod u32 {
    use super::*;

    pub struct U32Visitor;

    impl HasConstructor for U32Visitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for U32Visitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid u32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map_err(E::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value < 0.0
                || value > u32::MAX as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(value as u32)
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<u32>().map_err(E::custom)
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(u32::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U32Visitor)
    }
}

pub mod u32_opt {
    use super::*;

    struct U32Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for U32Visitor {
        type Value = Option<u32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid u32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value < 0.0
                || value > u32::MAX as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(Some(value as u32))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<u32>().map(Some).map_err(E::custom)
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U32Visitor)
    }
}

pub mod u64 {
    use super::*;

    pub struct U64Visitor;

    impl HasConstructor for U64Visitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> Visitor<'de> for U64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid u64")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as u64)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value < 0.0
                || value > u64::MAX as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number in the proper range, we can cast just fine.
                Ok(value as u64)
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<u64>().map_err(E::custom)
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(u64::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U64Visitor)
    }

    pub struct U64Serializer;

    impl SerializeMethod for U64Serializer {
        type Value = u64;
        #[cfg(feature = "std")]
        fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&value.to_string())
        }
    }
}

pub mod u64_opt {
    use super::*;

    struct U64Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for U64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid u64")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as u64))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if (value.trunc() - value).abs() > f64::EPSILON
                || value < 0.0
                || value > u64::MAX as f64
            {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                // This is a round number, we can cast just fine.
                Ok(Some(value as u64))
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // If we have scientific notation or a decimal, parse float first.
            if value.contains('e') || value.contains('E') || value.ends_with(".0") {
                value
                    .parse::<f64>()
                    .map_err(E::custom)
                    .and_then(|x| self.visit_f64(x))
            } else {
                value.parse::<u64>().map(Some).map_err(E::custom)
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U64Visitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(double) => u64::U64Serializer::serialize(double, serializer),
        }
    }
}

pub mod f64 {
    use super::*;

    pub struct F64Visitor;

    impl HasConstructor for F64Visitor {
        fn new() -> F64Visitor {
            F64Visitor {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for F64Visitor {
        type Value = f64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid f64")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as f64)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as f64)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "NaN" => Ok(f64::NAN),
                "Infinity" => Ok(f64::INFINITY),
                "-Infinity" => Ok(f64::NEG_INFINITY),
                _ => value.parse::<f64>().map_err(E::custom),
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(f64::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(F64Visitor)
    }

    pub struct F64Serializer;

    impl SerializeMethod for F64Serializer {
        type Value = f64;
        #[cfg(feature = "std")]
        fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            if value.is_nan() {
                serializer.serialize_str("NaN")
            } else if value.is_infinite() && value.is_sign_negative() {
                serializer.serialize_str("-Infinity")
            } else if value.is_infinite() {
                serializer.serialize_str("Infinity")
            } else {
                serializer.serialize_f64(*value)
            }
        }
    }
}

pub mod f64_opt {
    use super::*;

    struct F64Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for F64Visitor {
        type Value = Option<f64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid f64")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as f64))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as f64))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "NaN" => Ok(Some(f64::NAN)),
                "Infinity" => Ok(Some(f64::INFINITY)),
                "-Infinity" => Ok(Some(f64::NEG_INFINITY)),
                _ => value.parse::<f64>().map(Some).map_err(E::custom),
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(F64Visitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(double) => f64::F64Serializer::serialize(double, serializer),
        }
    }
}

pub mod f32 {
    use super::*;

    pub struct F32Visitor;

    impl HasConstructor for F32Visitor {
        fn new() -> F32Visitor {
            F32Visitor {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for F32Visitor {
        type Value = f32;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid f32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as f32)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < f32::MIN as f64 || value > f32::MAX as f64 {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                Ok(value as f32)
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as f32)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "NaN" => Ok(f32::NAN),
                "Infinity" => Ok(f32::INFINITY),
                "-Infinity" => Ok(f32::NEG_INFINITY),
                _ => value.parse::<f32>().map_err(E::custom),
            }
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(f32::default())
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(F32Visitor)
    }

    pub struct F32Serializer;

    impl SerializeMethod for F32Serializer {
        type Value = f32;

        #[cfg(feature = "std")]
        fn serialize<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            if value.is_nan() {
                serializer.serialize_str("NaN")
            } else if value.is_infinite() && value.is_sign_negative() {
                serializer.serialize_str("-Infinity")
            } else if value.is_infinite() {
                serializer.serialize_str("Infinity")
            } else {
                serializer.serialize_f32(*value)
            }
        }
    }
}

pub mod f32_opt {
    use super::*;

    struct F32Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for F32Visitor {
        type Value = Option<f32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid f32")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as f32))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < f32::MIN as f64 || value > f32::MAX as f64 {
                Err(de::Error::invalid_type(de::Unexpected::Float(value), &self))
            } else {
                Ok(Some(value as f32))
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value as f32))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value {
                "NaN" => Ok(Some(f32::NAN)),
                "Infinity" => Ok(Some(f32::INFINITY)),
                "-Infinity" => Ok(Some(f32::NEG_INFINITY)),
                _ => value.parse::<f32>().map(Some).map_err(E::custom),
            }
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(F32Visitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(value: &Option<f32>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(float) => f32::F32Serializer::serialize(float, serializer),
        }
    }
}

pub mod vec_u8 {
    use super::*;

    pub struct VecU8Visitor;

    impl HasConstructor for VecU8Visitor {
        fn new() -> Self {
            Self {}
        }
    }

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for VecU8Visitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid base64 encoded string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            base64::decode(value).map_err(E::custom)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Self::Value::default())
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(VecU8Visitor)
    }

    pub struct VecU8Serializer;

    impl SerializeMethod for VecU8Serializer {
        type Value = Vec<u8>;

        #[cfg(feature = "std")]
        fn serialize<S>(value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&base64::encode(value))
        }
    }
}

pub mod vec_u8_opt {
    use super::*;

    struct VecU8Visitor;

    #[cfg(feature = "std")]
    impl<'de> de::Visitor<'de> for VecU8Visitor {
        type Value = Option<Vec<u8>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid base64 encoded string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            base64::decode(value).map(Some).map_err(E::custom)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    #[cfg(feature = "std")]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(VecU8Visitor)
    }

    #[cfg(feature = "std")]
    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(value) => vec_u8::VecU8Serializer::serialize(value, serializer),
        }
    }
}
