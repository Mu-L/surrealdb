use crate::ctx::Context;
use crate::ctx::reason::Reason;
use crate::dbs::Force;
use crate::dbs::Options;
use crate::dbs::QueryType;
use crate::dbs::response::Response;
use crate::err::Error;
use crate::expr::Base;
use crate::expr::ControlFlow;
use crate::expr::FlowResult;
use crate::expr::paths::DB;
use crate::expr::paths::NS;
use crate::expr::statement::LogicalPlan;
use crate::expr::value::Value;
use crate::iam::Action;
use crate::iam::ResourceKind;
use crate::kvs::Datastore;
use crate::kvs::TransactionType;
use crate::kvs::{LockType, Transaction};
use crate::sql::planner::SqlToLogical;
use crate::sql::query::Query;
use crate::sql::statement::Statement;
use crate::sql::statements::{OptionStatement, UseStatement};
use anyhow::{Result, anyhow, bail};
use futures::{Stream, StreamExt};
use reblessive::TreeStack;
use std::fmt::Display;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_family = "wasm"))]
use tokio::spawn;
use tracing::{instrument, warn};
use trice::Instant;
#[cfg(target_family = "wasm")]
use wasm_bindgen_futures::spawn_local as spawn;

const TARGET: &str = "surrealdb::core::dbs";

pub struct Executor {
	stack: TreeStack,
	results: Vec<Response>,
	opt: Options,
	ctx: Context,
}

impl Executor {
	pub fn new(ctx: Context, opt: Options) -> Self {
		Executor {
			stack: TreeStack::new(),
			results: Vec::new(),
			opt,
			ctx,
		}
	}

	fn execute_use_statement(&mut self, stmt: UseStatement) -> Result<()> {
		let ctx_ref = Arc::get_mut(&mut self.ctx).ok_or_else(|| {
			Error::unreachable(format_args!("Tried to unfreeze a Context with multiple references"))
		})?;

		if let Some(ns) = stmt.ns {
			let mut session = ctx_ref.value("session").unwrap_or(&Value::None).clone();
			self.opt.set_ns(Some(ns.as_str().into()));
			session.put(NS.as_ref(), ns.into());
			ctx_ref.add_value("session", session.into());
		}
		if let Some(db) = stmt.db {
			let mut session = ctx_ref.value("session").unwrap_or(&Value::None).clone();
			self.opt.set_db(Some(db.as_str().into()));
			session.put(DB.as_ref(), db.into());
			ctx_ref.add_value("session", session.into());
		}
		Ok(())
	}

	fn execute_option_statement(&mut self, stmt: OptionStatement) -> Result<()> {
		// Allowed to run?
		self.opt.is_allowed(Action::Edit, ResourceKind::Option, &Base::Db)?;
		// Convert to uppercase
		let mut name = stmt.name.0;
		name.make_ascii_uppercase();
		// Process the option
		match name.as_str() {
			"IMPORT" => {
				self.opt.set_import(stmt.what);
			}
			"FORCE" => {
				let force = if stmt.what {
					Force::All
				} else {
					Force::None
				};
				self.opt.force = force;
			}
			"FUTURES" => {
				if stmt.what {
					self.opt.set_futures(true);
				} else {
					self.opt.set_futures_never();
				}
			}
			_ => {}
		};
		Ok(())
	}

	fn check_slow_log(&self, start: &Instant, stm: &impl Display) {
		if let Some(threshold) = self.ctx.slow_log_threshold() {
			let elapsed = start.elapsed();
			if elapsed > threshold {
				warn!("Slow query detected - time: {elapsed:#?} - query: {stm}")
			}
		}
	}

	/// Executes a statement which needs a transaction with the supplied transaction.
	#[instrument(level = "debug", name = "executor", target = "surrealdb::core::dbs", skip_all)]
	async fn execute_plan_in_transaction(
		&mut self,
		txn: Arc<Transaction>,
		start: &Instant,
		plan: LogicalPlan,
	) -> FlowResult<Value> {
		let res = match plan {
			LogicalPlan::Set(stm) => {
				// Avoid moving in and out of the context via Arc::get_mut
				Arc::get_mut(&mut self.ctx)
					.ok_or_else(|| {
						Error::unreachable("Tried to unfreeze a Context with multiple references")
					})
					.map_err(anyhow::Error::new)?
					.set_transaction(txn);
				// Run the statement
				let res = self
					.stack
					.enter(|stk| stm.compute(stk, &self.ctx, &self.opt, None))
					.finish()
					.await;
				// Check if we dump the slow log
				self.check_slow_log(start, &stm);
				match res {
					Ok(val) => {
						// Set the parameter
						Arc::get_mut(&mut self.ctx)
							.ok_or_else(|| {
								Error::unreachable(
									"Tried to unfreeze a Context with multiple references",
								)
							})
							.map_err(anyhow::Error::new)?
							.add_value(stm.name, val.into());
						// Finalise transaction, returning nothing unless it couldn't commit
						Ok(Value::None)
					}
					Err(err) => Err(err),
				}
			}
			// Process all other normal statements
			plan => {
				// The transaction began successfully
				Arc::get_mut(&mut self.ctx)
					.ok_or_else(|| {
						Error::unreachable("Tried to unfreeze a Context with multiple references")
					})
					.map_err(anyhow::Error::new)?
					.set_transaction(txn);
				// Process the statement
				let res = self
					.stack
					.enter(|stk| plan.compute(stk, &self.ctx, &self.opt, None))
					.finish()
					.await;
				self.check_slow_log(start, &plan);
				res
			}
		};

		// Catch cancellation during running.
		match self.ctx.done(true)? {
			None => res,
			Some(Reason::Timedout) => Err(ControlFlow::from(anyhow::anyhow!(Error::QueryTimedout))),
			Some(Reason::Canceled) => {
				Err(ControlFlow::from(anyhow::anyhow!(Error::QueryCancelled)))
			}
		}
	}

	/// Execute a query not wrapped in a transaction block.
	async fn execute_bare_statement(
		&mut self,
		kvs: &Datastore,
		start: &Instant,
		stmt: Statement,
	) -> Result<Value> {
		// Don't even try to run if the query should already be finished.
		match self.ctx.done(true)? {
			None => {}
			Some(Reason::Timedout) => {
				bail!(Error::QueryTimedout);
			}
			Some(Reason::Canceled) => {
				bail!(Error::QueryCancelled);
			}
		}

		match stmt {
			// These statements don't need a transaction.
			Statement::Use(stmt) => self.execute_use_statement(stmt).map(|_| Value::None),
			stmt => {
				let planner = SqlToLogical::new();
				let plan = planner.statement_to_logical(stmt)?;

				self.execute_plan_impl(kvs, start, plan).await
			}
		}
	}

	async fn execute_plan_impl(
		&mut self,
		kvs: &Datastore,
		start: &Instant,
		plan: LogicalPlan,
	) -> Result<Value> {
		let writeable = plan.writeable();
		let txn = Arc::new(kvs.transaction(writeable.into(), LockType::Optimistic).await?);
		let receiver = self.ctx.has_notifications().then(|| {
			let (send, recv) = async_channel::unbounded();
			self.opt.sender = Some(send);
			recv
		});

		match self.execute_plan_in_transaction(txn.clone(), start, plan).await {
			Ok(value) | Err(ControlFlow::Return(value)) => {
				let mut lock = txn.lock().await;

				// non-writable transactions might return an error on commit.
				// So cancel them instead. This is fine since a non-writable transaction
				// has nothing to commit anyway.
				if !writeable {
					let _ = lock.cancel().await;
					return Ok(value);
				}

				if let Err(e) = lock.complete_changes(false).await {
					let _ = lock.cancel().await;

					bail!(Error::QueryNotExecutedDetail {
						message: e.to_string(),
					});
				}

				if let Err(e) = lock.commit().await {
					bail!(Error::QueryNotExecutedDetail {
						message: e.to_string(),
					});
				}

				// flush notifications.
				if let Some(recv) = receiver {
					self.opt.sender = None;
					if let Some(sink) = self.ctx.notifications() {
						spawn(async move {
							while let Ok(x) = recv.recv().await {
								if sink.send(x).await.is_err() {
									break;
								}
							}
						});
					}
				}

				Ok(value)
			}
			Err(ControlFlow::Continue) | Err(ControlFlow::Break) => {
				bail!(Error::InvalidControlFlow)
			}
			Err(ControlFlow::Err(e)) => {
				let _ = txn.cancel().await;
				Err(e)
			}
		}
	}

	/// Execute the begin statement and all statements after which are within a transaction block.
	async fn execute_begin_statement<S>(
		&mut self,
		kvs: &Datastore,
		mut stream: Pin<&mut S>,
	) -> Result<()>
	where
		S: Stream<Item = Result<Statement>>,
	{
		let Ok(txn) = kvs.transaction(TransactionType::Write, LockType::Optimistic).await else {
			// couldn't create a transaction.
			// Fast forward until we hit CANCEL or COMMIT
			while let Some(stmt) = stream.next().await {
				yield_now!();
				let stmt = stmt?;
				if let Statement::Cancel(_) | Statement::Commit(_) = stmt {
					return Ok(());
				}

				self.results.push(Response {
					time: Duration::ZERO,
					result: Err(anyhow!(Error::QueryNotExecuted)),
					query_type: QueryType::Other,
				});
			}

			// Ran out of statements but still didn't hit a COMMIT or CANCEL
			// Just break as we can't do anything else since the query is already
			// effectively canceled.
			return Ok(());
		};

		// Create a sender for this transaction only if the context allows for notifications.
		let receiver = self.ctx.has_notifications().then(|| {
			let (send, recv) = async_channel::unbounded();
			self.opt.sender = Some(send);
			recv
		});

		let txn = Arc::new(txn);
		let start_results = self.results.len();
		let mut skip_remaining = false;

		// loop over the statements until we hit a cancel or a commit statement.
		while let Some(stmt) = stream.next().await {
			yield_now!();
			let stmt = match stmt {
				Ok(x) => x,
				Err(e) => {
					// make sure the transaction is properly canceled.
					let _ = txn.cancel().await;
					return Err(e);
				}
			};

			// check for timeout and cancellation.
			if let Some(done) = self.ctx.done(true)? {
				// A cancellation happened. Cancel the transaction, fast-forward the remaining
				// results and then return.
				let _ = txn.cancel().await;

				for res in &mut self.results[start_results..] {
					res.query_type = QueryType::Other;
					res.result = Err(anyhow!(Error::QueryCancelled));
				}

				while let Some(stmt) = stream.next().await {
					yield_now!();
					let stmt = stmt?;
					if let Statement::Cancel(_) | Statement::Commit(_) = stmt {
						return Ok(());
					}

					self.results.push(Response {
						time: Duration::ZERO,
						result: Err(match done {
							Reason::Timedout => anyhow!(Error::QueryTimedout),
							Reason::Canceled => anyhow!(Error::QueryCancelled),
						}),
						query_type: QueryType::Other,
					});
				}

				// Missing CANCEL/COMMIT statement, statement already canceled so nothing todo.
				return Ok(());
			}

			if skip_remaining && !matches!(stmt, Statement::Cancel(_) | Statement::Commit(_)) {
				continue;
			}

			trace!(target: TARGET, statement = %stmt, "Executing statement");

			let query_type = match stmt {
				Statement::Live(_) => QueryType::Live,
				Statement::Kill(_) => QueryType::Kill,
				_ => QueryType::Other,
			};

			let before = Instant::now();
			let value = match stmt {
				Statement::Begin(_) => {
					let _ = txn.cancel().await;
					// tried to begin a transaction within a transaction.

					for res in &mut self.results[start_results..] {
						res.query_type = QueryType::Other;
						res.result = Err(anyhow!(Error::QueryNotExecuted));
					}

					self.results.push(Response {
						time: Duration::ZERO,
						result: Err(anyhow!(Error::QueryNotExecutedDetail {
							message:
								"Tried to start a transaction while another transaction was open"
									.to_string(),
						})),
						query_type: QueryType::Other,
					});

					self.opt.sender = None;

					while let Some(stmt) = stream.next().await {
						yield_now!();
						let stmt = stmt?;
						if let Statement::Cancel(_) | Statement::Commit(_) = stmt {
							return Ok(());
						}

						self.results.push(Response {
							time: Duration::ZERO,
							result: Err(anyhow!(Error::QueryNotExecuted)),
							query_type: QueryType::Other,
						});
					}

					// Missing CANCEL/COMMIT statement, statement already canceled so nothing todo.
					return Ok(());
				}
				Statement::Cancel(_) => {
					let _ = txn.cancel().await;

					// update the results indicating cancelation.
					for res in &mut self.results[start_results..] {
						res.query_type = QueryType::Other;
						res.result = Err(anyhow!(Error::QueryCancelled));
					}

					self.opt.sender = None;

					return Ok(());
				}
				Statement::Commit(_) => {
					let mut lock = txn.lock().await;

					// complete_changes and then commit.
					// If either error undo results.
					let e = if let Err(e) = lock.complete_changes(false).await {
						let _ = lock.cancel().await;
						e
					} else if let Err(e) = lock.commit().await {
						e
					} else {
						// Successfully commited. everything is fine.

						// flush notifications.
						if let Some(recv) = receiver {
							self.opt.sender = None;
							if let Some(sink) = self.ctx.notifications() {
								spawn(async move {
									while let Ok(x) = recv.recv().await {
										if sink.send(x).await.is_err() {
											break;
										}
									}
								});
							}
						}

						return Ok(());
					};

					// failed to commit
					for res in &mut self.results[start_results..] {
						res.query_type = QueryType::Other;
						res.result = Err(anyhow!(Error::QueryNotExecutedDetail {
							message: e.to_string(),
						}));
					}

					self.opt.sender = None;

					return Ok(());
				}
				Statement::Option(stmt) => match self.execute_option_statement(stmt) {
					Ok(_) => {
						// skip adding the value as executing an option statement doesn't produce
						// results
						continue;
					}
					Err(e) => Err(e),
				},
				Statement::Use(stmt) => self.execute_use_statement(stmt).map(|_| Value::None),
				stmt => {
					skip_remaining = matches!(stmt, Statement::Output(_));

					let planner = SqlToLogical::new();
					let plan = planner.statement_to_logical(stmt)?;

					let r = match self.execute_plan_in_transaction(txn.clone(), &before, plan).await
					{
						Ok(x) => Ok(x),
						Err(ControlFlow::Return(value)) => {
							skip_remaining = true;
							Ok(value)
						}
						Err(ControlFlow::Break) | Err(ControlFlow::Continue) => {
							Err(anyhow!(Error::InvalidControlFlow))
						}
						Err(ControlFlow::Err(e)) => {
							for res in &mut self.results[start_results..] {
								res.query_type = QueryType::Other;
								res.result = Err(anyhow!(Error::QueryNotExecuted));
							}

							// statement return an error. Consume all the other statement until we hit a cancel or commit.
							self.results.push(Response {
								time: before.elapsed(),
								result: Err(e),
								query_type,
							});

							let _ = txn.cancel().await;

							self.opt.sender = None;

							while let Some(stmt) = stream.next().await {
								yield_now!();
								let stmt = stmt?;
								if let Statement::Cancel(_) | Statement::Commit(_) = stmt {
									return Ok(());
								}

								self.results.push(Response {
									time: Duration::ZERO,
									result: Err(anyhow!(Error::QueryNotExecuted)),
									query_type: QueryType::Other,
								});
							}

							// ran out of statements before the transaction ended.
							// Just break as we have nothing else we can do.
							return Ok(());
						}
					};

					if skip_remaining {
						// If we skip the next values due to return then we need to clear the other
						// results.
						self.results.truncate(start_results)
					}

					r
				}
			};

			self.results.push(Response {
				time: before.elapsed(),
				result: value,
				query_type,
			});
		}

		// we ran out of query but we still have an open transaction.
		// Be conservative and treat this essentially as a CANCEL statement.
		let _ = txn.cancel().await;

		for res in &mut self.results[start_results..] {
			res.query_type = QueryType::Other;
			res.result = Err(anyhow!(Error::QueryNotExecutedDetail {
				message: "Missing COMMIT statement".to_string(),
			}));
		}

		self.opt.sender = None;

		Ok(())
	}

	#[instrument(level = "debug", name = "executor", target = "surrealdb::core::dbs", skip_all)]
	pub async fn execute(
		kvs: &Datastore,
		ctx: Context,
		opt: Options,
		qry: Query,
	) -> Result<Vec<Response>> {
		let stream = futures::stream::iter(qry.into_iter().map(Ok));
		Self::execute_stream(kvs, ctx, opt, false, stream).await
	}

	pub async fn execute_plan(
		kvs: &Datastore,
		ctx: Context,
		opt: Options,
		plan: LogicalPlan,
	) -> Result<Vec<Response>> {
		let mut this = Executor::new(ctx, opt);

		let query_type = match &plan {
			LogicalPlan::Live(_) => QueryType::Live,
			LogicalPlan::Kill(_) => QueryType::Kill,
			_ => QueryType::Other,
		};

		let now = Instant::now();
		let result = this.execute_plan_impl(kvs, &now, plan).await;

		Ok(vec![Response {
			time: now.elapsed(),
			result,
			query_type,
		}])
	}

	#[instrument(level = "debug", name = "executor", target = "surrealdb::core::dbs", skip_all)]
	pub async fn execute_stream<S>(
		kvs: &Datastore,
		ctx: Context,
		opt: Options,
		skip_success_results: bool,
		stream: S,
	) -> Result<Vec<Response>>
	where
		S: Stream<Item = Result<Statement>>,
	{
		let mut this = Executor::new(ctx, opt);
		let mut stream = pin!(stream);

		while let Some(stmt) = stream.next().await {
			yield_now!();
			let stmt = match stmt {
				Ok(x) => x,
				Err(e) => {
					this.results.push(Response {
						time: Duration::ZERO,
						result: Err(e),
						query_type: QueryType::Other,
					});

					return Ok(this.results);
				}
			};

			match stmt {
				Statement::Option(stmt) => this.execute_option_statement(stmt)?,
				// handle option here because it doesn't produce a result.
				Statement::Begin(_) => {
					if let Err(e) = this.execute_begin_statement(kvs, stream.as_mut()).await {
						this.results.push(Response {
							time: Duration::ZERO,
							result: Err(e),
							query_type: QueryType::Other,
						});

						return Ok(this.results);
					}
				}
				stmt => {
					let query_type: QueryType = (&stmt).into();

					let now = Instant::now();
					let result = this.execute_bare_statement(kvs, &now, stmt).await;
					if !skip_success_results || result.is_err() {
						this.results.push(Response {
							time: now.elapsed(),
							result,
							query_type,
						});
					}
				}
			}
		}
		Ok(this.results)
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		dbs::Session,
		iam::{Level, Role},
		kvs::Datastore,
	};

	#[tokio::test]
	async fn check_execute_option_permissions() {
		let tests = vec![
			// Root level
			(
				Session::for_level(Level::Root, Role::Owner).with_ns("NS").with_db("DB"),
				true,
				"owner at root level should be able to set options",
			),
			(
				Session::for_level(Level::Root, Role::Editor).with_ns("NS").with_db("DB"),
				true,
				"editor at root level should be able to set options",
			),
			(
				Session::for_level(Level::Root, Role::Viewer).with_ns("NS").with_db("DB"),
				false,
				"viewer at root level should not be able to set options",
			),
			// Namespace level
			(
				Session::for_level(Level::Namespace("NS".to_string()), Role::Owner)
					.with_ns("NS")
					.with_db("DB"),
				true,
				"owner at namespace level should be able to set options on its namespace",
			),
			(
				Session::for_level(Level::Namespace("NS".to_string()), Role::Owner)
					.with_ns("OTHER_NS")
					.with_db("DB"),
				false,
				"owner at namespace level should not be able to set options on another namespace",
			),
			(
				Session::for_level(Level::Namespace("NS".to_string()), Role::Editor)
					.with_ns("NS")
					.with_db("DB"),
				true,
				"editor at namespace level should be able to set options on its namespace",
			),
			(
				Session::for_level(Level::Namespace("NS".to_string()), Role::Editor)
					.with_ns("OTHER_NS")
					.with_db("DB"),
				false,
				"editor at namespace level should not be able to set options on another namespace",
			),
			(
				Session::for_level(Level::Namespace("NS".to_string()), Role::Viewer)
					.with_ns("NS")
					.with_db("DB"),
				false,
				"viewer at namespace level should not be able to set options on its namespace",
			),
			// Database level
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Owner,
				)
				.with_ns("NS")
				.with_db("DB"),
				true,
				"owner at database level should be able to set options on its database",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Owner,
				)
				.with_ns("NS")
				.with_db("OTHER_DB"),
				false,
				"owner at database level should not be able to set options on another database",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Owner,
				)
				.with_ns("OTHER_NS")
				.with_db("DB"),
				false,
				"owner at database level should not be able to set options on another namespace even if the database name matches",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Editor,
				)
				.with_ns("NS")
				.with_db("DB"),
				true,
				"editor at database level should be able to set options on its database",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Editor,
				)
				.with_ns("NS")
				.with_db("OTHER_DB"),
				false,
				"editor at database level should not be able to set options on another database",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Editor,
				)
				.with_ns("OTHER_NS")
				.with_db("DB"),
				false,
				"editor at database level should not be able to set options on another namespace even if the database name matches",
			),
			(
				Session::for_level(
					Level::Database("NS".to_string(), "DB".to_string()),
					Role::Viewer,
				)
				.with_ns("NS")
				.with_db("DB"),
				false,
				"viewer at database level should not be able to set options on its database",
			),
		];
		let statement = "OPTION IMPORT = false";

		for test in tests.iter() {
			let (session, should_succeed, msg) = test;

			{
				let ds = Datastore::new("memory").await.unwrap().with_auth_enabled(true);

				let res = ds.execute(statement, session, None).await;

				if *should_succeed {
					assert!(res.is_ok(), "{}: {:?}", msg, res);
				} else {
					let err = res.unwrap_err().to_string();
					assert!(
						err.contains("Not enough permissions to perform this action"),
						"{}: {}",
						msg,
						err
					)
				}
			}
		}

		// Anonymous with auth enabled
		{
			let ds = Datastore::new("memory").await.unwrap().with_auth_enabled(true);

			let res =
				ds.execute(statement, &Session::default().with_ns("NS").with_db("DB"), None).await;

			let err = res.unwrap_err().to_string();
			assert!(
				err.contains("Not enough permissions to perform this action"),
				"anonymous user should not be able to set options: {}",
				err
			)
		}

		// Anonymous with auth disabled
		{
			let ds = Datastore::new("memory").await.unwrap().with_auth_enabled(false);

			let res =
				ds.execute(statement, &Session::default().with_ns("NS").with_db("DB"), None).await;

			assert!(
				res.is_ok(),
				"anonymous user should be able to set options when auth is disabled: {:?}",
				res
			)
		}
	}

	#[tokio::test]
	async fn check_execute_timeout() {
		// With small timeout
		{
			let ds = Datastore::new("memory").await.unwrap();
			let stmt = "UPDATE test TIMEOUT 2s";
			let res = ds.execute(stmt, &Session::default().with_ns("NS").with_db("DB"), None).await;
			assert!(res.is_ok(), "Failed to execute statement with small timeout: {:?}", res);
		}
		// With large timeout
		{
			let ds = Datastore::new("memory").await.unwrap();
			let stmt = "UPDATE test TIMEOUT 31540000s"; // 1 year
			let res = ds.execute(stmt, &Session::default().with_ns("NS").with_db("DB"), None).await;
			assert!(res.is_ok(), "Failed to execute statement with large timeout: {:?}", res);
		}
		// With very large timeout
		{
			let ds = Datastore::new("memory").await.unwrap();
			let stmt = "UPDATE test TIMEOUT 9460800000000000000s"; // 300 billion years
			let res = ds.execute(stmt, &Session::default().with_ns("NS").with_db("DB"), None).await;
			assert!(res.is_ok(), "Failed to execute statement with very large timeout: {:?}", res);
			let err = res.unwrap()[0].result.as_ref().unwrap_err().to_string();
			assert!(
				err.contains("Invalid timeout"),
				"Expected to find invalid timeout error: {:?}",
				err
			);
		}
	}
}
