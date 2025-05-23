use super::escape::EscapeKey;
use super::{
	Array, Bytes, Closure, Datetime, Duration, File, Geometry, Ident, Idiom, Number, Object, Part,
	Range, Regex, Strand, Thing, Uuid,
};
use crate::sql::statements::info::InfoStructure;
use crate::sql::{
	fmt::{is_pretty, pretty_indent, Fmt, Pretty},
	Table, Value,
};
use geo::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use revision::revisioned;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter, Write};

#[revisioned(revision = 2)]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[non_exhaustive]
pub enum Kind {
	Any,
	Null,
	Bool,
	Bytes,
	Datetime,
	Decimal,
	Duration,
	Float,
	Int,
	Number,
	Object,
	Point,
	String,
	Uuid,
	#[revision(start = 2)]
	Regex,
	Record(Vec<Table>),
	Geometry(Vec<String>),
	Option(Box<Kind>),
	Either(Vec<Kind>),
	Set(Box<Kind>, Option<u64>),
	Array(Box<Kind>, Option<u64>),
	Function(Option<Vec<Kind>>, Option<Box<Kind>>),
	Range,
	Literal(Literal),
	References(Option<Table>, Option<Idiom>),
	/// If the kind was specified without a bucket the vec will be empty.
	/// So `<file>` is just `Kind::File(Vec::new())`
	File(Vec<Ident>),
}

impl Default for Kind {
	fn default() -> Self {
		Self::Any
	}
}

impl Kind {
	/// Returns the kind of a type.
	pub(crate) fn of<T: HasKind>() -> Kind {
		T::kind()
	}

	/// Returns true if this type is an `any`
	pub(crate) fn is_any(&self) -> bool {
		matches!(self, Kind::Any)
	}

	/// Returns true if this type is a record
	pub(crate) fn is_record(&self) -> bool {
		matches!(self, Kind::Record(_))
	}

	/// Returns true if this type is optional
	pub(crate) fn can_be_none(&self) -> bool {
		matches!(self, Kind::Option(_) | Kind::Any)
	}

	/// Returns the kind in case of a literal, otherwise returns the kind itself
	fn to_non_literal_kind(&self) -> Self {
		match self {
			Kind::Literal(l) => l.to_kind(),
			k => k.to_owned(),
		}
	}

	/// Returns true if this type is a literal, or contains a literal
	pub(crate) fn contains_literal(&self) -> bool {
		if matches!(self, Kind::Literal(_)) {
			return true;
		}

		if let Kind::Option(x) = self {
			return x.contains_literal();
		}

		if let Kind::Either(x) = self {
			return x.iter().any(|x| x.contains_literal());
		}

		false
	}

	/// Returns Some if this type can be converted into a discriminated object, None otherwise
	pub(crate) fn to_discriminated(&self) -> Option<Kind> {
		match self {
			Kind::Either(nested) => {
				if let Some(nested) = nested
					.iter()
					.map(|k| match k {
						Kind::Literal(Literal::Object(o)) => Some(o),
						_ => None,
					})
					.collect::<Option<Vec<&BTreeMap<String, Kind>>>>()
				{
					if let Some(first) = nested.first() {
						let mut key: Option<String> = None;

						'key: for (k, v) in first.iter() {
							let mut kinds: Vec<Kind> = vec![v.to_owned()];
							for item in nested[1..].iter() {
								if let Some(kind) = item.get(k) {
									match kind {
										Kind::Literal(l)
											if kinds.contains(&l.to_kind())
												|| kinds.contains(&Kind::Literal(l.to_owned())) =>
										{
											continue 'key;
										}
										kind if kinds
											.iter()
											.any(|k| *kind == k.to_non_literal_kind()) =>
										{
											continue 'key;
										}
										kind => {
											kinds.push(kind.to_owned());
										}
									}
								} else {
									continue 'key;
								}
							}

							key = Some(k.clone());
							break;
						}

						if let Some(key) = key {
							return Some(Kind::Literal(Literal::DiscriminatedObject(
								key.clone(),
								nested.into_iter().map(|o| o.to_owned()).collect(),
							)));
						}
					}
				}

				None
			}
			_ => None,
		}
	}

	// Return the kind of the contained value.
	//
	// For example: for `array<number>` or `set<number>` this returns `number`.
	// For `array<number> | set<float>` this returns `number | float`.
	pub(crate) fn inner_kind(&self) -> Option<Kind> {
		let mut this = self;
		loop {
			match &this {
				Kind::Any
				| Kind::Null
				| Kind::Bool
				| Kind::Bytes
				| Kind::Datetime
				| Kind::Decimal
				| Kind::Duration
				| Kind::Float
				| Kind::Int
				| Kind::Number
				| Kind::Object
				| Kind::Point
				| Kind::String
				| Kind::Uuid
				| Kind::Regex
				| Kind::Record(_)
				| Kind::Geometry(_)
				| Kind::Function(_, _)
				| Kind::Range
				| Kind::Literal(_)
				| Kind::References(_, _)
				| Kind::File(_) => return None,
				Kind::Option(x) => {
					this = x;
				}
				Kind::Array(x, _) | Kind::Set(x, _) => return Some(x.as_ref().clone()),
				Kind::Either(x) => {
					// a either shouldn't be able to contain a either itself so recursing here
					// should be fine.
					let kinds: Vec<Kind> = x.iter().filter_map(Self::inner_kind).collect();
					if kinds.is_empty() {
						return None;
					}
					return Some(Kind::Either(kinds));
				}
			}
		}
	}

	pub(crate) fn non_optional(&self) -> &Kind {
		match self {
			Kind::Option(k) => k.as_ref().non_optional(),
			_ => self,
		}
	}

	pub(crate) fn allows_nested_kind(&self, path: &[Part], kind: &Kind) -> bool {
		// ANY type won't cause a mismatch
		if self.is_any() || kind.is_any() {
			return true;
		}

		if !path.is_empty() {
			match self {
				Kind::Object => return matches!(path.first(), Some(Part::Field(_) | Part::All)),
				Kind::Either(kinds) => {
					return kinds.iter().all(|k| k.allows_nested_kind(path, kind))
				}
				Kind::Array(inner, len) | Kind::Set(inner, len) => {
					return match path.first() {
						Some(Part::All) => inner.allows_nested_kind(&path[1..], kind),
						Some(Part::Index(i)) => {
							if let Some(len) = len {
								if i.as_usize() >= *len as usize {
									return false;
								}
							}

							inner.allows_nested_kind(&path[1..], kind)
						}
						_ => false,
					}
				}
				_ => (),
			}
		}

		match self {
			Kind::Literal(lit) => lit.allows_nested_kind(path, kind),
			Kind::Option(inner) => inner.allows_nested_kind(path, kind),
			_ if path.is_empty() => self == kind,
			_ => false,
		}
	}
}

/// Trait for retrieving the `kind` equivalent of a rust type.
///
/// Returns the most general kind for a type.
/// For example Number could be either number or float or int or decimal but the most general is
/// number.
///
/// This trait is only implemented for types which can only be retrieve from
pub trait HasKind {
	fn kind() -> Kind;
}

impl<T: HasKind> HasKind for Option<T> {
	fn kind() -> Kind {
		let kind = T::kind();
		if matches!(kind, Kind::Option(_)) {
			kind
		} else {
			Kind::Option(Box::new(kind))
		}
	}
}

impl<T: HasKind> HasKind for Vec<T> {
	fn kind() -> Kind {
		let kind = T::kind();
		Kind::Array(Box::new(kind), None)
	}
}

impl HasKind for Array {
	fn kind() -> Kind {
		Kind::Array(Box::new(Kind::Any), None)
	}
}

impl<T: HasKind, const SIZE: usize> HasKind for [T; SIZE] {
	fn kind() -> Kind {
		let kind = T::kind();
		Kind::Array(Box::new(kind), Some(SIZE as u64))
	}
}

impl HasKind for Thing {
	fn kind() -> Kind {
		Kind::Record(Vec::new())
	}
}

impl HasKind for Geometry {
	fn kind() -> Kind {
		Kind::Geometry(Vec::new())
	}
}

impl HasKind for Closure {
	fn kind() -> Kind {
		// The inner values of function are currently completely unused.
		Kind::Function(None, None)
	}
}

impl HasKind for Regex {
	fn kind() -> Kind {
		Kind::Regex
	}
}

impl HasKind for File {
	fn kind() -> Kind {
		Kind::File(Vec::new())
	}
}

macro_rules! impl_basic_has_kind{
	($($name:ident => $kind:ident),*$(,)?) => {
		$(
			impl HasKind for $name{
				fn kind() -> Kind{
					Kind::$kind
				}
			}
		)*
	}
}

impl_basic_has_kind! {
	bool => Bool,

	i64 => Int,
	f64 => Float,
	Decimal => Decimal,

	String => String,
	Strand => String,
	Bytes => Bytes,
	Number => Number,
	Datetime => Datetime,
	Duration => Duration,
	Uuid => Uuid,
	Object => Object,
	Range => Range,
}

macro_rules! impl_geometry_has_kind{
	($($name:ty => $kind:literal),*$(,)?) => {
		$(
			impl HasKind for $name{
				fn kind() -> Kind{
					Kind::Geometry(vec![$kind.to_string()])
				}
			}
		)*
	}
}
impl_geometry_has_kind! {
	Point<f64> => "point",
	LineString<f64> => "line",
	MultiPoint<f64> => "multipoint",
	Polygon<f64> => "polygon",
	MultiLineString<f64> => "multiline",
	MultiPolygon<f64> => "multipolygon",
}

impl From<&Kind> for Box<Kind> {
	#[inline]
	fn from(v: &Kind) -> Self {
		Box::new(v.clone())
	}
}

impl Display for Kind {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		match self {
			Kind::Any => f.write_str("any"),
			Kind::Null => f.write_str("null"),
			Kind::Bool => f.write_str("bool"),
			Kind::Bytes => f.write_str("bytes"),
			Kind::Datetime => f.write_str("datetime"),
			Kind::Decimal => f.write_str("decimal"),
			Kind::Duration => f.write_str("duration"),
			Kind::Float => f.write_str("float"),
			Kind::Int => f.write_str("int"),
			Kind::Number => f.write_str("number"),
			Kind::Object => f.write_str("object"),
			Kind::Point => f.write_str("point"),
			Kind::String => f.write_str("string"),
			Kind::Uuid => f.write_str("uuid"),
			Kind::Regex => f.write_str("regex"),
			Kind::Function(_, _) => f.write_str("function"),
			Kind::Option(k) => write!(f, "option<{}>", k),
			Kind::Record(k) => {
				if k.is_empty() {
					write!(f, "record")
				} else {
					write!(f, "record<{}>", Fmt::verbar_separated(k))
				}
			}
			Kind::Geometry(k) => {
				if k.is_empty() {
					write!(f, "geometry")
				} else {
					write!(f, "geometry<{}>", Fmt::verbar_separated(k))
				}
			}
			Kind::Set(k, l) => match (k, l) {
				(k, None) if k.is_any() => write!(f, "set"),
				(k, None) => write!(f, "set<{k}>"),
				(k, Some(l)) => write!(f, "set<{k}, {l}>"),
			},
			Kind::Array(k, l) => match (k, l) {
				(k, None) if k.is_any() => write!(f, "array"),
				(k, None) => write!(f, "array<{k}>"),
				(k, Some(l)) => write!(f, "array<{k}, {l}>"),
			},
			Kind::Either(k) => write!(f, "{}", Fmt::verbar_separated(k)),
			Kind::Range => f.write_str("range"),
			Kind::Literal(l) => write!(f, "{}", l),
			Kind::References(t, i) => match (t, i) {
				(Some(t), None) => write!(f, "references<{}>", t),
				(Some(t), Some(i)) => write!(f, "references<{}, {}>", t, i),
				(None, _) => f.write_str("references"),
			},
			Kind::File(k) => {
				if k.is_empty() {
					write!(f, "file")
				} else {
					write!(f, "file<{}>", Fmt::verbar_separated(k))
				}
			}
		}
	}
}

impl InfoStructure for Kind {
	fn structure(self) -> Value {
		self.to_string().into()
	}
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[non_exhaustive]
pub enum Literal {
	String(Strand),
	Number(Number),
	Duration(Duration),
	Array(Vec<Kind>),
	Object(BTreeMap<String, Kind>),
	DiscriminatedObject(String, Vec<BTreeMap<String, Kind>>),
	Bool(bool),
}

impl Literal {
	pub fn to_kind(&self) -> Kind {
		match self {
			Self::String(_) => Kind::String,
			Self::Number(_) => Kind::Number,
			Self::Duration(_) => Kind::Duration,
			Self::Array(a) => {
				if let Some(inner) = a.first() {
					if a.iter().all(|x| x == inner) {
						return Kind::Array(Box::new(inner.to_owned()), Some(a.len() as u64));
					}
				}

				Kind::Array(Box::new(Kind::Any), None)
			}
			Self::Object(_) => Kind::Object,
			Self::DiscriminatedObject(_, _) => Kind::Object,
			Self::Bool(_) => Kind::Bool,
		}
	}

	pub fn validate_value(&self, value: &Value) -> bool {
		match self {
			Self::String(v) => match value {
				Value::Strand(s) => s == v,
				_ => false,
			},
			Self::Number(v) => match value {
				Value::Number(n) => n == v,
				_ => false,
			},
			Self::Duration(v) => match value {
				Value::Duration(n) => n == v,
				_ => false,
			},
			Self::Bool(v) => match value {
				Value::Bool(b) => b == v,
				_ => false,
			},
			Self::Array(a) => match value {
				Value::Array(x) => {
					if a.len() != x.len() {
						return false;
					}

					for (i, inner) in a.iter().enumerate() {
						if let Some(value) = x.get(i) {
							if value.to_owned().coerce_to_kind(inner).is_err() {
								return false;
							}
						} else {
							return false;
						}
					}

					true
				}
				_ => false,
			},
			Self::Object(o) => match value {
				Value::Object(x) => {
					if o.len() < x.len() {
						return false;
					}

					for (k, v) in o.iter() {
						if let Some(value) = x.get(k) {
							if value.to_owned().coerce_to_kind(v).is_err() {
								return false;
							}
						} else if !v.can_be_none() {
							return false;
						}
					}

					true
				}
				_ => false,
			},
			Self::DiscriminatedObject(key, discriminants) => match value {
				Value::Object(x) => {
					let value = x.get(key).unwrap_or(&Value::None);
					if let Some(o) = discriminants
						.iter()
						.find(|o| value.to_owned().coerce_to_kind(&o[key]).is_ok())
					{
						if o.len() < x.len() {
							return false;
						}

						for (k, v) in o.iter() {
							if let Some(value) = x.get(k) {
								if value.to_owned().coerce_to_kind(v).is_err() {
									return false;
								}
							} else if !v.can_be_none() {
								return false;
							}
						}

						true
					} else {
						false
					}
				}
				_ => false,
			},
		}
	}

	pub(crate) fn allows_nested_kind(&self, path: &[Part], kind: &Kind) -> bool {
		// ANY type won't cause a mismatch
		if kind.is_any() {
			return true;
		}

		// We reached the end of the path
		// Check if the literal is equal to the kind
		if path.is_empty() {
			return match kind {
				Kind::Literal(lit) => self == lit,
				_ => &self.to_kind() == kind,
			};
		}

		match self {
			Literal::Array(x) => match path.first() {
				Some(Part::All) => x.iter().all(|y| y.allows_nested_kind(&path[1..], kind)),
				Some(Part::Index(i)) => {
					if let Some(y) = x.get(i.as_usize()) {
						y.allows_nested_kind(&path[1..], kind)
					} else {
						false
					}
				}
				_ => false,
			},
			Literal::Object(x) => match path.first() {
				Some(Part::All) => x.iter().all(|(_, y)| y.allows_nested_kind(&path[1..], kind)),
				Some(Part::Field(k)) => {
					if let Some(y) = x.get(&k.0) {
						y.allows_nested_kind(&path[1..], kind)
					} else {
						false
					}
				}
				_ => false,
			},
			Literal::DiscriminatedObject(_, discriminants) => match path.first() {
				Some(Part::All) => discriminants
					.iter()
					.all(|o| o.iter().all(|(_, y)| y.allows_nested_kind(&path[1..], kind))),
				Some(Part::Field(k)) => discriminants.iter().all(|o| {
					if let Some(y) = o.get(&k.0) {
						y.allows_nested_kind(&path[1..], kind)
					} else {
						false
					}
				}),
				_ => false,
			},
			_ => false,
		}
	}
}

impl Display for Literal {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		match self {
			Literal::String(s) => write!(f, "{}", s),
			Literal::Number(n) => write!(f, "{}", n),
			Literal::Duration(d) => write!(f, "{}", d),
			Literal::Bool(b) => write!(f, "{}", b),
			Literal::Array(a) => {
				let mut f = Pretty::from(f);
				f.write_char('[')?;
				if !a.is_empty() {
					let indent = pretty_indent();
					write!(f, "{}", Fmt::pretty_comma_separated(a.as_slice()))?;
					drop(indent);
				}
				f.write_char(']')
			}
			Literal::Object(o) => {
				let mut f = Pretty::from(f);
				if is_pretty() {
					f.write_char('{')?;
				} else {
					f.write_str("{ ")?;
				}
				if !o.is_empty() {
					let indent = pretty_indent();
					write!(
						f,
						"{}",
						Fmt::pretty_comma_separated(o.iter().map(|args| Fmt::new(
							args,
							|(k, v), f| write!(f, "{}: {}", EscapeKey(k), v)
						)),)
					)?;
					drop(indent);
				}
				if is_pretty() {
					f.write_char('}')
				} else {
					f.write_str(" }")
				}
			}
			Literal::DiscriminatedObject(_, discriminants) => {
				let mut f = Pretty::from(f);

				for (i, o) in discriminants.iter().enumerate() {
					if i > 0 {
						f.write_str(" | ")?;
					}

					if is_pretty() {
						f.write_char('{')?;
					} else {
						f.write_str("{ ")?;
					}
					if !o.is_empty() {
						let indent = pretty_indent();
						write!(
							f,
							"{}",
							Fmt::pretty_comma_separated(o.iter().map(|args| Fmt::new(
								args,
								|(k, v), f| write!(f, "{}: {}", EscapeKey(k), v)
							)),)
						)?;
						drop(indent);
					}
					if is_pretty() {
						f.write_char('}')?;
					} else {
						f.write_str(" }")?;
					}
				}

				Ok(())
			}
		}
	}
}
