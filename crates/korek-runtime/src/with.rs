//! Composable serde adapters for fields whose JSON form differs from their
//! Rust type at any depth: `#[serde(with = "::korek_runtime::with::As::<C>")]`
//! where `C` is a [`Codec`] such as [`crate::base64::Base64`],
//! `List<Base64>` (`list<bytes>`), `Map<List<Base64>>` or `Opt<C>` (an
//! optional field).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How a `T` is written to and read from its JSON form.
pub trait Codec<T> {
    /// # Errors
    /// From the serializer.
    fn serialize<S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error>;

    /// # Errors
    /// When the input is not in the codec's form.
    fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<T, D::Error>;
}

/// The `with` target for a codec: `#[serde(with = "…::with::As::<C>")]`.
pub struct As<C>(PhantomData<C>);

impl<C> As<C> {
    /// # Errors
    /// From the serializer.
    pub fn serialize<T, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error>
    where
        C: Codec<T>,
    {
        C::serialize(v, s)
    }

    /// # Errors
    /// When the input is not in the codec's form.
    pub fn deserialize<'de, T, D: Deserializer<'de>>(d: D) -> Result<T, D::Error>
    where
        C: Codec<T>,
    {
        C::deserialize(d)
    }
}

/// `Vec<T>` with each item through `C`.
pub struct List<C>(PhantomData<C>);

/// `BTreeMap<String, T>` with each value through `C`.
pub struct Map<C>(PhantomData<C>);

/// `Option<T>` with the value through `C`.
pub struct Opt<C>(PhantomData<C>);

struct Ser<'a, C, T>(&'a T, PhantomData<C>);

impl<C: Codec<T>, T> Serialize for Ser<'_, C, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        C::serialize(self.0, s)
    }
}

fn ser<C, T>(v: &T) -> Ser<'_, C, T> {
    Ser(v, PhantomData)
}

struct De<C, T>(T, PhantomData<C>);

impl<'de, C: Codec<T>, T> Deserialize<'de> for De<C, T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        C::deserialize(d).map(|v| De(v, PhantomData))
    }
}

impl<C: Codec<T>, T> Codec<Vec<T>> for List<C> {
    fn serialize<S: Serializer>(v: &Vec<T>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(ser::<C, T>))
    }

    fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<T>, D::Error> {
        let v = Vec::<De<C, T>>::deserialize(d)?;
        Ok(v.into_iter().map(|x| x.0).collect())
    }
}

impl<C: Codec<T>, T> Codec<BTreeMap<String, T>> for Map<C> {
    fn serialize<S: Serializer>(v: &BTreeMap<String, T>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(v.iter().map(|(k, x)| (k, ser::<C, T>(x))))
    }

    fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, T>, D::Error> {
        let m = BTreeMap::<String, De<C, T>>::deserialize(d)?;
        Ok(m.into_iter().map(|(k, x)| (k, x.0)).collect())
    }
}

impl<C: Codec<T>, T> Codec<Option<T>> for Opt<C> {
    fn serialize<S: Serializer>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(x) => s.serialize_some(&ser::<C, T>(x)),
            None => s.serialize_none(),
        }
    }

    fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<T>, D::Error> {
        Ok(Option::<De<C, T>>::deserialize(d)?.map(|x| x.0))
    }
}
