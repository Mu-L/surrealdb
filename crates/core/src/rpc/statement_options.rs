use crate::{
	dbs::{Capabilities, Variables},
	sql::{Cond, Data, Fetchs, Fields, Limit, Number, Output, SqlValue, Start, Timeout, Version},
	syn::{
		fetchs_with_capabilities, fields_with_capabilities, output_with_capabilities,
		value_with_capabilities,
	},
};

use super::RpcError;

#[derive(Clone, Debug)]
pub(crate) enum RpcData {
	Patch(SqlValue),
	Merge(SqlValue),
	Replace(SqlValue),
	Content(SqlValue),
	Single(SqlValue),
}

impl From<RpcData> for Data {
	fn from(data: RpcData) -> Self {
		match data {
			RpcData::Patch(v) => Data::PatchExpression(v),
			RpcData::Merge(v) => Data::MergeExpression(v),
			RpcData::Replace(v) => Data::ReplaceExpression(v),
			RpcData::Content(v) => Data::ContentExpression(v),
			RpcData::Single(v) => Data::SingleExpression(v),
		}
	}
}

impl RpcData {
	pub(crate) fn value(&self) -> &SqlValue {
		match self {
			RpcData::Patch(v) => v,
			RpcData::Merge(v) => v,
			RpcData::Replace(v) => v,
			RpcData::Content(v) => v,
			RpcData::Single(v) => v,
		}
	}

	pub(crate) fn from_string(str: String, v: SqlValue) -> Result<RpcData, RpcError> {
		match str.to_lowercase().as_str() {
			"patch" => Ok(RpcData::Patch(v)),
			"merge" => Ok(RpcData::Merge(v)),
			"replace" => Ok(RpcData::Replace(v)),
			"content" => Ok(RpcData::Content(v)),
			"single" => Ok(RpcData::Single(v)),
			_ => Err(RpcError::InvalidParams),
		}
	}
}

/// Statement Options for the `select`, `insert`, `create`, `upsert`, `update`, `relate` and `delete` methods.
#[derive(Clone, Debug, Default)]
pub(crate) struct StatementOptions {
	/// - One of: `"content"`, `"replace"`, `"merge"`, `"patch"` or `"single"`.
	/// - For the `insert`, `create`, `upsert`, `update` and `relate` methods
	pub data: Option<RpcData>,
	/// - A string, containing fields to select. Also works with the `VALUE` keyword.
	/// - For the `select` method
	pub fields: Option<Fields>,
	/// - One of: `"none"`, `"null"`, `"diff"`, `"before"`, `"after"` or a list of fields
	/// - For the `insert`, `create`, `upsert`, `update`, `relate` and `delete` methods
	pub output: Option<Output>,
	/// - A number, stating how many records can be selected or affected
	/// - For the `select` method
	pub limit: Option<Limit>,
	/// - A number, stating how many records to skip in a selection
	/// - For the `select` method
	pub start: Option<Start>,
	/// - A string, containing an expression for a `WHERE` clause
	/// - For the `select`, `upsert`, `update` and `delete` methods
	pub cond: Option<Cond>,
	/// - A boolean, stating where we want to select or affect only a single record.
	/// - For the `select`, `create`, `upsert`, `update`, `relate` and `delete` methods
	pub only: bool,
	/// - A boolean, stating wether we are inserting relations.
	/// - For the `insert` method
	pub relation: bool,
	/// - A boolean, stating wether the relation we are inserting needs to be unique
	/// - For the `relate` method
	pub unique: bool,
	/// - Can contain either:
	///    - A datetime
	///    - A string, containing an expression which computes into a datetime
	/// - For the `select`, `insert` and `create` methods
	pub version: Option<Version>,
	/// - A duration, stating how long execution can last
	/// - For all (`select`, `insert`, `create`, `upsert`, `update`, `relate` and `delete`) methods
	pub timeout: Option<Timeout>,
	/// - An object, containing variables to define during execution of the method
	/// - For all (`select`, `insert`, `create`, `upsert`, `update`, `relate` and `delete`) methods
	pub vars: Option<Variables>,
	/// - A boolean, stating wether the LQ notifications should contain diffs
	/// - For the `live` method
	pub diff: bool,
	/// - A string, containing fields to fetch.
	/// - For the `select` and `live` methods
	pub fetch: Option<Fetchs>,
}

impl StatementOptions {
	pub(crate) fn with_data_content(&mut self, v: SqlValue) -> &mut Self {
		self.data = Some(RpcData::Content(v));
		self
	}

	pub(crate) fn with_output(&mut self, output: Output) -> &mut Self {
		self.output = Some(output);
		self
	}

	pub(crate) fn process_options(
		&mut self,
		opts: SqlValue,
		capabilities: &Capabilities,
	) -> Result<&mut Self, RpcError> {
		if let SqlValue::Object(mut obj) = opts {
			// Process "data_expr" option
			if let Some(data) = &self.data {
				if let Some(v) = obj.remove("data_expr") {
					if let SqlValue::Strand(v) = v {
						self.data =
							Some(RpcData::from_string(v.to_string(), data.value().to_owned())?);
					} else {
						return Err(RpcError::InvalidParams);
					}
				}
			}

			// Process "fields" option
			if let Some(v) = obj.remove("fields") {
				if let SqlValue::Strand(v) = v {
					self.fields = Some(fields_with_capabilities(v.as_str(), capabilities)?)
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "return" option
			if let Some(v) = obj.remove("return") {
				if let SqlValue::Strand(v) = v {
					self.output = Some(output_with_capabilities(v.as_str(), capabilities)?)
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "limit" option
			if let Some(v) = obj.remove("limit") {
				if let SqlValue::Number(Number::Int(_)) = v {
					self.limit = Some(Limit(v))
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "start" option
			if let Some(v) = obj.remove("start") {
				if let SqlValue::Number(Number::Int(_)) = v {
					self.start = Some(Start(v))
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "cond" option
			if let Some(v) = obj.remove("cond") {
				if let SqlValue::Strand(v) = v {
					let v = value_with_capabilities(v.as_str(), capabilities)?;
					self.cond = Some(Cond(v))
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "version" option
			if let Some(v) = obj.remove("version") {
				let v = match v {
					v @ SqlValue::Datetime(_) => v,
					SqlValue::Strand(v) => value_with_capabilities(v.as_str(), capabilities)?,
					_ => {
						return Err(RpcError::InvalidParams);
					}
				};

				self.version = Some(Version(v))
			}

			// Process "timeout" option
			if let Some(v) = obj.remove("timeout") {
				if let SqlValue::Duration(v) = v {
					self.timeout = Some(Timeout(v))
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "only" option
			if let Some(v) = obj.remove("only") {
				if let SqlValue::Bool(v) = v {
					self.only = v;
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "relation" option
			if let Some(v) = obj.remove("relation") {
				if let SqlValue::Bool(v) = v {
					self.relation = v;
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "unique" option
			if let Some(v) = obj.remove("unique") {
				if let SqlValue::Bool(v) = v {
					self.unique = v;
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "vars" option
			if let Some(v) = obj.remove("vars") {
				if let SqlValue::Object(v) = v {
					let v: crate::expr::Object = v.into();
					self.vars = Some(v.into())
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "diff" option
			if let Some(v) = obj.remove("diff") {
				if self.fields.is_some() {
					// diff and fields cannot co-exist, as diff overwrites the fields
					return Err(RpcError::InvalidParams);
				}

				if let SqlValue::Bool(v) = v {
					self.diff = v;
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			// Process "fetch" option
			if let Some(v) = obj.remove("fetch") {
				if let SqlValue::Strand(v) = v {
					self.fetch = Some(fetchs_with_capabilities(v.as_str(), capabilities)?)
				} else {
					return Err(RpcError::InvalidParams);
				}
			}

			Ok(self)
		} else {
			Err(RpcError::InvalidParams)
		}
	}

	pub(crate) fn data_expr(&self) -> Option<Data> {
		self.data.clone().map(|v| v.into())
	}

	pub(crate) fn merge_vars(&self, v: &Variables) -> Variables {
		match &self.vars {
			Some(vars) => vars.merged(v.clone()),
			None => v.clone(),
		}
	}
}
