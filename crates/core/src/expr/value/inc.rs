use crate::expr::number::Number;
use crate::expr::part::Part;
use crate::expr::value::Value;

impl Value {
	/// Synchronous method for incrementing a field in a `Value`
	pub(crate) fn inc(&mut self, path: &[Part], val: Value) {
		match self.pick(path) {
			Value::Number(v) => {
				if let Value::Number(x) = val {
					self.put(path, Value::from(v + x))
				}
			}
			Value::Array(v) => match val {
				Value::Array(x) => self.put(path, Value::from(v + x)),
				x => self.put(path, Value::from(v + x)),
			},
			Value::None => match val {
				Value::Number(x) => self.put(path, Value::from(Number::from(0) + x)),
				Value::Array(x) => self.put(path, Value::from(x)),
				x => self.put(path, Value::from(vec![x])),
			},
			_ => (),
		}
	}
}

#[cfg(test)]
mod tests {

	use super::*;
	use crate::expr::idiom::Idiom;

	use crate::sql::idiom::Idiom as SqlIdiom;
	use crate::syn::Parse;

	#[tokio::test]
	async fn increment_none() {
		let idi: Idiom = SqlIdiom::parse("other").into();
		let mut val: Value = Value::parse("{ test: 100 }");
		let res: Value = Value::parse("{ test: 100, other: +10 }");
		val.inc(&idi, Value::from(10));
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn increment_number() {
		let idi: Idiom = SqlIdiom::parse("test").into();
		let mut val: Value = Value::parse("{ test: 100 }");
		let res: Value = Value::parse("{ test: 110 }");
		val.inc(&idi, Value::from(10));
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn increment_array_number() {
		let idi: Idiom = SqlIdiom::parse("test[1]").into();
		let mut val: Value = Value::parse("{ test: [100, 200, 300] }");
		let res: Value = Value::parse("{ test: [100, 210, 300] }");
		val.inc(&idi, Value::from(10));
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn increment_array_value() {
		let idi: Idiom = SqlIdiom::parse("test").into();
		let mut val: Value = Value::parse("{ test: [100, 200, 300] }");
		let res: Value = Value::parse("{ test: [100, 200, 300, 200] }");
		val.inc(&idi, Value::from(200));
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn increment_array_array() {
		let idi: Idiom = SqlIdiom::parse("test").into();
		let mut val: Value = Value::parse("{ test: [100, 200, 300] }");
		let res: Value = Value::parse("{ test: [100, 200, 300, 100, 300, 400, 500] }");
		val.inc(&idi, Value::parse("[100, 300, 400, 500]"));
		assert_eq!(res, val);
	}
}
