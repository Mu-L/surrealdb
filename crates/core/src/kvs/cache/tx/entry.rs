use crate::dbs::node::Node;
use crate::expr::Value;
use crate::expr::statements::AccessGrant;
use crate::expr::statements::DefineAccessStatement;
use crate::expr::statements::DefineAnalyzerStatement;
use crate::expr::statements::DefineDatabaseStatement;
use crate::expr::statements::DefineEventStatement;
use crate::expr::statements::DefineFieldStatement;
use crate::expr::statements::DefineFunctionStatement;
use crate::expr::statements::DefineIndexStatement;
use crate::expr::statements::DefineModelStatement;
use crate::expr::statements::DefineNamespaceStatement;
use crate::expr::statements::DefineParamStatement;
use crate::expr::statements::DefineTableStatement;
use crate::expr::statements::DefineUserStatement;
use crate::expr::statements::LiveStatement;
use crate::expr::statements::define::DefineConfigStatement;
use crate::expr::statements::define::{ApiDefinition, BucketDefinition, DefineSequenceStatement};
use anyhow::Result;
use std::any::Any;
use std::sync::Arc;

#[derive(Clone)]
#[non_exhaustive]
pub(crate) enum Entry {
	/// A cached entry of any type
	Any(Arc<dyn Any + Send + Sync>),
	/// A cached record document content
	Val(Arc<Value>),
	/// A slice of Node specified at the root.
	Nds(Arc<[Node]>),
	/// A slice of DefineUserStatement specified at the root.
	Rus(Arc<[DefineUserStatement]>),
	/// A slice of DefineAccessStatement specified at the root.
	Ras(Arc<[DefineAccessStatement]>),
	/// A slice of AccessGrant specified at the root.
	Rag(Arc<[AccessGrant]>),
	/// A slice of DefineNamespaceStatement specified on a namespace.
	Nss(Arc<[DefineNamespaceStatement]>),
	/// A slice of DefineUserStatement specified on a namespace.
	Nus(Arc<[DefineUserStatement]>),
	/// A slice of DefineAccessStatement specified on a namespace.
	Nas(Arc<[DefineAccessStatement]>),
	/// A slice of AccessGrant specified at on a namespace.
	Nag(Arc<[AccessGrant]>),
	/// A slice of DefineDatabaseStatement specified on a namespace.
	Dbs(Arc<[DefineDatabaseStatement]>),
	/// A slice of ApiDefinition specified on a namespace.
	Aps(Arc<[ApiDefinition]>),
	/// A slice of DefineAnalyzerStatement specified on a namespace.
	Azs(Arc<[DefineAnalyzerStatement]>),
	/// A slice of DefineBucketStatement specified on a database.
	Bus(Arc<[BucketDefinition]>),
	/// A slice of DefineAccessStatement specified on a database.
	Das(Arc<[DefineAccessStatement]>),
	/// A slice of AccessGrant specified at on a database.
	Dag(Arc<[AccessGrant]>),
	/// A slice of DefineUserStatement specified on a database.
	Dus(Arc<[DefineUserStatement]>),
	/// A slice of DefineFunctionStatement specified on a database.
	Fcs(Arc<[DefineFunctionStatement]>),
	/// A slice of DefineTableStatement specified on a database.
	Tbs(Arc<[DefineTableStatement]>),
	/// A slice of DefineModelStatement specified on a database.
	Mls(Arc<[DefineModelStatement]>),
	/// A slice of DefineConfigStatement specified on a database.
	Cgs(Arc<[DefineConfigStatement]>),
	/// A slice of DefineParamStatement specified on a database.
	Pas(Arc<[DefineParamStatement]>),
	/// A slice of DefineSequenceStatement specified on a namespace.
	Sqs(Arc<[DefineSequenceStatement]>),
	/// A slice of DefineEventStatement specified on a table.
	Evs(Arc<[DefineEventStatement]>),
	/// A slice of DefineFieldStatement specified on a table.
	Fds(Arc<[DefineFieldStatement]>),
	/// A slice of DefineTableStatement specified on a table.
	Fts(Arc<[DefineTableStatement]>),
	/// A slice of DefineIndexStatement specified on a table.
	Ixs(Arc<[DefineIndexStatement]>),
	/// A slice of LiveStatement specified on a table.
	Lvs(Arc<[LiveStatement]>),
}

impl Entry {
	/// Converts this cache entry into a single entry of arbitrary type.
	/// This panics if called on a cache entry that is not an [`Entry::Any`].
	pub(crate) fn try_into_type<T: Send + Sync + 'static>(self: Entry) -> Result<Arc<T>> {
		match self {
			Entry::Any(v) => {
				let Ok(x) = v.downcast::<T>() else {
					fail!("Unable to convert type into Entry::Any")
				};
				Ok(x)
			}
			_ => fail!("Unable to convert type into Entry::Any"),
		}
	}
	/// Converts this cache entry into a slice of [`Node`].
	/// This panics if called on a cache entry that is not an [`Entry::Nds`].
	pub(crate) fn try_into_nds(self) -> Result<Arc<[Node]>> {
		match self {
			Entry::Nds(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Nds"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineUserStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Rus`].
	pub(crate) fn try_into_rus(self) -> Result<Arc<[DefineUserStatement]>> {
		match self {
			Entry::Rus(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Rus"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineAccessStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Ras`].
	pub(crate) fn try_into_ras(self) -> Result<Arc<[DefineAccessStatement]>> {
		match self {
			Entry::Ras(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Ras"),
		}
	}
	/// Converts this cache entry into a slice of [`AccessGrant`].
	/// This panics if called on a cache entry that is not an [`Entry::Rag`].
	pub(crate) fn try_into_rag(self) -> Result<Arc<[AccessGrant]>> {
		match self {
			Entry::Rag(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Rag"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineNamespaceStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Nss`].
	pub(crate) fn try_into_nss(self) -> Result<Arc<[DefineNamespaceStatement]>> {
		match self {
			Entry::Nss(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Nss"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineAccessStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Nas`].
	pub(crate) fn try_into_nas(self) -> Result<Arc<[DefineAccessStatement]>> {
		match self {
			Entry::Nas(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Nas"),
		}
	}
	/// Converts this cache entry into a slice of [`AccessGrant`].
	/// This panics if called on a cache entry that is not an [`Entry::Nag`].
	pub(crate) fn try_into_nag(self) -> Result<Arc<[AccessGrant]>> {
		match self {
			Entry::Nag(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Nag"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineUserStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Nus`].
	pub(crate) fn try_into_nus(self) -> Result<Arc<[DefineUserStatement]>> {
		match self {
			Entry::Nus(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Nus"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineDatabaseStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Dbs`].
	pub(crate) fn try_into_dbs(self) -> Result<Arc<[DefineDatabaseStatement]>> {
		match self {
			Entry::Dbs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Dbs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineAccessStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Das`].
	pub(crate) fn try_into_das(self) -> Result<Arc<[DefineAccessStatement]>> {
		match self {
			Entry::Das(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Das"),
		}
	}
	/// Converts this cache entry into a slice of [`AccessGrant`].
	/// This panics if called on a cache entry that is not an [`Entry::Dag`].
	pub(crate) fn try_into_dag(self) -> Result<Arc<[AccessGrant]>> {
		match self {
			Entry::Dag(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Dag"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineUserStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Dus`].
	pub(crate) fn try_into_dus(self) -> Result<Arc<[DefineUserStatement]>> {
		match self {
			Entry::Dus(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Dus"),
		}
	}
	/// Converts this cache entry into a slice of [`ApiDefinition`].
	/// This panics if called on a cache entry that is not an [`Entry::Aps`].
	pub(crate) fn try_into_aps(self) -> Result<Arc<[ApiDefinition]>> {
		match self {
			Entry::Aps(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Aps"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineAnalyzerStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Azs`].
	pub(crate) fn try_into_azs(self) -> Result<Arc<[DefineAnalyzerStatement]>> {
		match self {
			Entry::Azs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Azs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineBucketStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Bus`].
	pub(crate) fn try_into_bus(self) -> Result<Arc<[BucketDefinition]>> {
		match self {
			Entry::Bus(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Bus"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineSequenceStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Sqs`].
	pub(crate) fn try_into_sqs(self) -> Result<Arc<[DefineSequenceStatement]>> {
		match self {
			Entry::Sqs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Sqs"),
		}
	}

	/// Converts this cache entry into a slice of [`DefineFunctionStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Fcs`].
	pub(crate) fn try_into_fcs(self) -> Result<Arc<[DefineFunctionStatement]>> {
		match self {
			Entry::Fcs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Fcs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineParamStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Pas`].
	pub(crate) fn try_into_pas(self) -> Result<Arc<[DefineParamStatement]>> {
		match self {
			Entry::Pas(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Pas"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineModelStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Mls`].
	pub(crate) fn try_into_mls(self) -> Result<Arc<[DefineModelStatement]>> {
		match self {
			Entry::Mls(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Mls"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineConfigStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Cgs`].
	pub(crate) fn try_into_cgs(self) -> Result<Arc<[DefineConfigStatement]>> {
		match self {
			Entry::Cgs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Cgs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineTableStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Tbs`].
	pub(crate) fn try_into_tbs(self) -> Result<Arc<[DefineTableStatement]>> {
		match self {
			Entry::Tbs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Tbs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineEventStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Evs`].
	pub(crate) fn try_into_evs(self) -> Result<Arc<[DefineEventStatement]>> {
		match self {
			Entry::Evs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Evs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineFieldStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Fds`].
	pub(crate) fn try_into_fds(self) -> Result<Arc<[DefineFieldStatement]>> {
		match self {
			Entry::Fds(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Fds"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineIndexStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Ixs`].
	pub(crate) fn try_into_ixs(self) -> Result<Arc<[DefineIndexStatement]>> {
		match self {
			Entry::Ixs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Ixs"),
		}
	}
	/// Converts this cache entry into a slice of [`DefineTableStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Fts`].
	pub(crate) fn try_into_fts(self) -> Result<Arc<[DefineTableStatement]>> {
		match self {
			Entry::Fts(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Fts"),
		}
	}
	/// Converts this cache entry into a slice of [`LiveStatement`].
	/// This panics if called on a cache entry that is not an [`Entry::Lvs`].
	pub(crate) fn try_into_lvs(self) -> Result<Arc<[LiveStatement]>> {
		match self {
			Entry::Lvs(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Lvs"),
		}
	}
	/// Converts this cache entry into a single [`Value`].
	/// This panics if called on a cache entry that is not an [`Entry::Val`].
	pub(crate) fn try_into_val(self) -> Result<Arc<Value>> {
		match self {
			Entry::Val(v) => Ok(v),
			_ => fail!("Unable to convert type into Entry::Val"),
		}
	}
}
