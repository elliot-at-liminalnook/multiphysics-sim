//! Validate serialized numeric values before JSON can replace NaN/infinity with
//! null. Walking Serialize also checks optional floats and future model fields.
use serde::{
    Serialize, Serializer,
    ser::{self, Error},
};

pub(crate) fn to_value(value: &impl Serialize) -> std::result::Result<serde_json::Value, String> {
    value.serialize(Finite).map_err(|e| e.to_string())?;
    serde_json::to_value(value).map_err(|e| e.to_string())
}

struct Finite;
struct Compound(usize);
type Result<T = ()> = std::result::Result<T, serde_json::Error>;

macro_rules! scalar {
    ($($name:ident($ty:ty)),* $(,)?) => {$ (
        fn $name(self, _: $ty) -> Result { Ok(()) }
    )*};
}

impl Serializer for Finite {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Compound;
    type SerializeTuple = Compound;
    type SerializeTupleStruct = Compound;
    type SerializeTupleVariant = Compound;
    type SerializeMap = Compound;
    type SerializeStruct = Compound;
    type SerializeStructVariant = Compound;

    scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8])
    );
    fn serialize_f32(self, value: f32) -> Result {
        self.serialize_f64(value as f64)
    }
    fn serialize_f64(self, value: f64) -> Result {
        if value.is_finite() {
            Ok(())
        } else {
            Err(Self::Error::custom(
                "nonfinite number cannot be preserved in JSON",
            ))
        }
    }
    fn serialize_none(self) -> Result {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Result {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result {
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_tuple(self, _: usize) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Compound> {
        Ok(Compound(0))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Compound> {
        Ok(Compound(0))
    }
}

impl Compound {
    fn item<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        let index = self.0;
        self.0 += 1;
        value
            .serialize(Finite)
            .map_err(|e| serde_json::Error::custom(format!("[{index}]: {e}")))
    }
    fn field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result {
        value
            .serialize(Finite)
            .map_err(|e| serde_json::Error::custom(format!("{key}: {e}")))
    }
}
macro_rules! sequence {
    ($($trait:ident, $method:ident);* $(;)?) => {$ (
        impl ser::$trait for Compound {
            type Ok = ();
            type Error = serde_json::Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result { self.item(value) }
            fn end(self) -> Result { Ok(()) }
        }
    )*};
}
sequence!(SerializeSeq, serialize_element; SerializeTuple, serialize_element;
    SerializeTupleStruct, serialize_field; SerializeTupleVariant, serialize_field);
macro_rules! structure {
    ($($trait:ident),*) => {$ (
        impl ser::$trait for Compound {
            type Ok = ();
            type Error = serde_json::Error;
            fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result { self.field(key, value) }
            fn end(self) -> Result { Ok(()) }
        }
    )*};
}
structure!(SerializeStruct, SerializeStructVariant);
impl ser::SerializeMap for Compound {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result {
        self.item(key)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        self.item(value)
    }
    fn end(self) -> Result {
        Ok(())
    }
}
