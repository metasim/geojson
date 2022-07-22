use std::convert::{TryFrom, TryInto};
use std::fmt::Formatter;
use std::io::Read;
use std::marker::PhantomData;

use crate::Result;

use serde::de::{Deserialize, Deserializer, Error, IntoDeserializer};

use crate::JsonValue;

pub struct FeatureCollectionVisitor;

impl FeatureCollectionVisitor {
    fn new() -> Self {
        Self
    }
}

impl<'de> serde::de::Visitor<'de> for FeatureCollectionVisitor {
    type Value = Vec<JsonValue>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "a valid GeoJSON Feature object")
    }

    fn visit_map<A>(self, mut map_access: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut has_feature_collection_type = false;
        let mut features = None;
        while let Some((key, value)) = map_access.next_entry::<String, JsonValue>()? {
            if key == "type" {
                if value == JsonValue::String("FeatureCollection".to_string()) {
                    has_feature_collection_type = true;
                } else {
                    return Err(A::Error::custom("invalid type for feature collection"));
                }
            } else if key == "features" {
                if let JsonValue::Array(value) = value {
                    if features.is_some() {
                        return Err(A::Error::custom(
                            "Encountered more than one list of `features`",
                        ));
                    }
                    features = Some(value);
                } else {
                    return Err(A::Error::custom("`features` had unexpected value"));
                }
            } else {
                return Err(A::Error::custom(
                    "foreign members are not handled by FeatureCollection deserializer",
                ));
            }
        }

        if let Some(features) = features {
            if has_feature_collection_type {
                Ok(features)
            } else {
                Err(A::Error::custom("No `type` field was found"))
            }
        } else {
            Err(A::Error::custom("No `features` field was found"))
        }
    }
}

struct FeatureVisitor<D> {
    _marker: PhantomData<D>,
}

impl<D> FeatureVisitor<D> {
    fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<'de, D> serde::de::Visitor<'de> for FeatureVisitor<D>
where
    D: Deserialize<'de>,
{
    type Value = D;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "a valid GeoJSON Feature object")
    }

    fn visit_map<A>(self, mut map_access: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut has_feature_type = false;
        use std::collections::HashMap;
        let mut hash_map: HashMap<String, crate::JsonValue> = HashMap::new();

        while let Some((key, value)) = map_access.next_entry::<String, JsonValue>()? {
            if key == "type" {
                if value.as_str() == Some("Feature") {
                    has_feature_type = true;
                } else {
                    return Err(A::Error::custom(
                        "GeoJSON Feature had a `type` other than \"Feature\"",
                    ));
                }
            } else if key == "geometry" {
                if let JsonValue::Object(_) = value {
                    hash_map.insert("geometry".to_string(), value);
                } else {
                    return Err(A::Error::custom(
                        "GeoJSON Feature had a unexpected geometry",
                    ));
                }
            } else if key == "properties" {
                if let JsonValue::Object(properties) = value {
                    // flatten properties onto struct
                    for (prop_key, prop_value) in properties {
                        hash_map.insert(prop_key, prop_value);
                    }
                } else {
                    return Err(A::Error::custom(
                        "GeoJSON Feature had a unexpected geometry",
                    ));
                }
            } else {
                return Err(A::Error::custom(
                    "foreign members are not handled by FeatureCollection deserializer",
                ));
            }
        }

        if has_feature_type {
            // What do I actually do here? serde-transcode? or create a new MapAccess or Struct that
            // has the fields needed by a child visitor - perhaps using serde::de::value::MapAccessDeserializer?
            // use serde::de::value::MapAccessDeserializer;
            let d2 = hash_map.into_deserializer();
            Ok(serde::Deserialize::deserialize(d2).expect("TODO"))
        } else {
            return Err(A::Error::custom(
                "A GeoJSON Feature must have a `type: \"Feature\"` field, but found none.",
            ));
        }
    }
}

pub(crate) fn deserialize_collection_features_from_reader<'de, D>(
    feature_collection_reader: impl Read,
) -> Result<impl Iterator<Item = Result<D>>>
where
    D: Deserialize<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_reader(feature_collection_reader);

    // TODO: rather than deserializing the entirety of the `features:` array into memory here, it'd
    // be nice to stream the features. However, I ran into difficulty while trying to return any
    // borrowed reference from the visitor methods (e.g. MapAccess)
    let visitor = FeatureCollectionVisitor::new();
    let objects = deserializer.deserialize_map(visitor)?;

    Ok(objects.into_iter().map(|feature_value| {
        let deserializer = feature_value.into_deserializer();
        let visitor = FeatureVisitor::new();
        let record: D = deserializer.deserialize_map(visitor)?;

        Ok(record)
    }))
}

pub fn deserialize_geometry<'de, D, G>(deserializer: D) -> std::result::Result<G, D::Error>
where
    D: serde::de::Deserializer<'de>,
    G: TryFrom<crate::Geometry>,
    G::Error: std::fmt::Display,
{
    let geojson_geometry = crate::Geometry::deserialize(deserializer)?;
    geojson_geometry.try_into().map_err(|err| {
        D::Error::custom(format!("unable to convert from geojson Geometry: {}", err))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[cfg(feature = "geo-types")]
    mod geo_types_tests {
        use super::*;

        // Some example object, that we want to parse the geojson into.
        #[derive(Deserialize)]
        struct MyStruct {
            // TODO: try to parse into specific variants like Point
            #[serde(deserialize_with = "deserialize_geometry")]
            geometry: geo_types::Geometry<f64>,
            name: String,
            age: u64,
        }

        fn feature_collection_string() -> String {
            json!({
                "type": "FeatureCollection",
                "features": [
                    {
                      "type": "Feature",
                      "geometry": {
                        "type": "Point",
                        "coordinates": [125.6, 10.1]
                      },
                      "properties": {
                        "name": "Dinagat Islands",
                        "age": 123
                      }
                    },
                    {
                      "type": "Feature",
                      "geometry": {
                        "type": "Point",
                        "coordinates": [2.3, 4.5]
                      },
                      "properties": {
                        "name": "Neverland",
                        "age": 456
                      }
                    }
                ]
            })
            .to_string()
        }

        #[test]
        fn feature_collection_from_reader() {
            let feature_collection_string = feature_collection_string();
            let bytes_reader = feature_collection_string.as_bytes();

            let records: Vec<MyStruct> = deserialize_collection_features_from_reader(bytes_reader)
                .expect("a valid feature collection")
                .map(|result| result.expect("a valid feature"))
                .collect();

            assert_eq!(records.len(), 2);

            assert_eq!(
                records[0].geometry,
                geo_types::point!(x: 125.6, y: 10.1).into()
            );
            assert_eq!(records[0].name, "Dinagat Islands");
            assert_eq!(records[0].age, 123);

            assert_eq!(
                records[1].geometry,
                geo_types::point!(x: 2.3, y: 4.5).into()
            );
            assert_eq!(records[1].name, "Neverland");
            assert_eq!(records[1].age, 456);
        }
    }
}
