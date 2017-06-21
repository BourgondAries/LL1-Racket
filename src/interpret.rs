//! Evaluation and library functions.
//!
//! ```
//! extern crate teko;
//! extern crate num_traits;
//! use num_traits::cast::ToPrimitive;
//! fn main() {
//! 	let program = teko::parse::parse_string("(+ 1 2 4) (+ 1 2)").ok().unwrap();
//! 	let env = teko::interpret::interpret(program);
//! 	match env.result.1 {
//! 		teko::data_structures::Coredata::Integer(ref value) => {
//! 			assert_eq![value.to_i32().unwrap(), 3];
//! 		}
//! 		_ => {
//! 			panic!["Expected Integer but got a different data type"];
//! 		}
//! 	}
//! }
//! ```
use std::rc::Rc;
use super::VEC_CAPACITY;

use num::bigint::BigInt;

use builtins::*;
use data_structures::{Boolean, Commands, Env, Program, Sourcedata, Coredata, Macro, Function};
use utilities::*;

/// Evaluates a program with a given environment.
///
/// The `program` is considered completely evaluated when it is empty. The result of the program
/// is stored in `env.result`. This function is mainly used to evaluate a program in some
/// environment context.
///
/// ```
/// extern crate teko;
/// extern crate num_traits;
/// use num_traits::cast::ToPrimitive;
/// fn main() {
/// 	let program = teko::parse::parse_string("(+ 1 2 4) (+ 1 2)").ok().unwrap();
/// 		let env = teko::interpret::initialize_environment_with_standard_library();
/// 	let env = teko::interpret::eval(program, env);
/// 	match env.result.1 {
/// 		teko::data_structures::Coredata::Integer(ref value) => {
/// 			assert_eq![value.to_i32().unwrap(), 3];
/// 		}
/// 		_ => {
/// 			panic!["Expected Integer but got a different data type"];
/// 		}
/// 	}
/// }
/// ```
pub fn eval(mut program: Program, mut env: Env) -> Env {
	while let Some(top) = program.pop() {
		match &*top {
			// Source refers to the head of the pair from which the call originated
			&Sourcedata(ref source, Coredata::Internal(Commands::Call(ref statement))) => {
				match &**statement {
					&Sourcedata(_, Coredata::Function(Function::Builtin(ref transfer))) => {
						let error = transfer(&mut program, &mut env);
						if let Some(_) = env.params.pop() {
							// Do nothing
						} else {
							unwind_with_error_message("during builtin function call: parameter \
							                           stack not poppable",
							                          &mut program,
							                          &mut env);
						}
						if let Some(error) = error {
							if let &Some(ref source) = source {
								trace(&mut program, &mut env);
								unwind_with_error_message(&format!["{} <= {}", error, source][..],
								                          &mut program,
								                          &mut env);
							}
						}
					}
					&Sourcedata(_,
					            Coredata::Function(Function::Library(ref parameters,
					                                                 ref transfer))) => {
						if let Some(arguments) = env.params.pop() {
							if arguments.len() != parameters.len() {
								unwind_with_error_message("during library function call: arity \
								                           mismatch",
								                          &mut program,
								                          &mut env);
							} else {
								let mut counter = 0;
								let cmd =
									Commands::Deparameterize(optimize_tail_call(&mut program,
									                                            &mut env,
									                                            parameters));
								for parameter in parameters.iter() {
									if env.store.contains_key(parameter) {
										env.store
											.get_mut(parameter)
											.unwrap()
											.push(arguments[counter].clone());
									} else {
										env.store.insert(parameter.clone(),
										                 vec![arguments[counter].clone()]);
									}
									counter += 1;
								}
								let next = Rc::new(Sourcedata(source.clone(),
								                              Coredata::Internal(cmd)));
								program.push(next);
								program.extend(transfer.iter().cloned());
							}
						} else {
							unwind_with_error_message("during library function call: parameter \
							                           stack empty",
							                          &mut program,
							                          &mut env);
						}
					}
					_ => {
						unwind_with_error_message("calling: Element not recognized as callable",
						                          &mut program,
						                          &mut env);
					}
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Deparameterize(ref arguments))) => {
				pop_parameters(&mut program, &mut env, arguments);
			}
			&Sourcedata(_, Coredata::Internal(Commands::Evaluate)) => {
				program.push(env.result.clone());
			}
			&Sourcedata(_, Coredata::Internal(Commands::If(ref first, ref second))) => {
				if let Coredata::Boolean(Boolean::False) = env.result.1 {
					program.push(second.clone());
				} else {
					program.push(first.clone());
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Parameterize)) => {
				let succeeded = if let Some(ref mut last) = env.params.last_mut() {
					last.push(env.result.clone());
					false
				} else {
					true
				};
				if succeeded {
					// Is there any point in unwinding if the param stack is not consistent?
					// What CAN we do and what can't we do? Suppose a builtin doesn't work
					// as intended, should the entire program crash? Should the interpreter just
					// halt? Maybe, or we can unwind, but can we reset the parameter stack?
					unwind_with_error_message("Error during parameterization: the parameter \
					                           stack is nonexistent",
					                          &mut program,
					                          &mut env);
				}
			}
			// Source here is the HEAD of a pair, so (a b) has source of a, and (((a)) b) has source of ((a))
			&Sourcedata(ref source, Coredata::Internal(Commands::Prepare(ref arguments))) => {
				match &*env.result.clone() {
					&Sourcedata(_, Coredata::Function(..)) => {
						env.params.push(vec![]);
						program.push(Rc::new(Sourcedata(source.clone(),
						                                Coredata::Internal(Commands::Call(env.result
							                                .clone())))));
						for argument in collect_pair_into_vec(arguments).iter() {
							program.push(Rc::new(Sourcedata(None,
							                                Coredata::Internal(Commands::Parameterize))));
							program.push(argument.clone());
						}
					}
					&Sourcedata(_, Coredata::Macro(Macro::Builtin(ref transfer))) => {
						env.result = arguments.clone();
						transfer(&mut program, &mut env);
						// TODO What do we do if it returns Some? Unwind!
					}
					&Sourcedata(_, Coredata::Macro(Macro::Library(ref bound, ref code))) => {
						program.push(Rc::new(Sourcedata(None,
						                                Coredata::Internal(Commands::Evaluate))));
						let command =
							optimize_tail_call(&mut program, &mut env, &vec![bound.clone()]);
						if env.store.contains_key(bound) {
							env.store.get_mut(bound).unwrap().push(arguments.clone());
						} else {
							env.store.insert(bound.clone(), vec![arguments.clone()]);
						}
						let deparam = Coredata::Internal(Commands::Deparameterize(command));
						let next = Rc::new(Sourcedata(source.clone(), deparam));
						program.push(next);
						program.extend(code.iter().cloned());
					}
					_ => {
						unwind_with_error_message(&format!["Error during prepare routine: \
						                                    element not callable => {:?}",
						                                   source],
						                          &mut program,
						                          &mut env);
					}
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Wind)) => {
				// Do nothing
			}
			// Maybe use pair start as source?
			&Sourcedata(_, Coredata::Pair(ref head, ref tail)) => {
				program.push(Rc::new(Sourcedata(head.0.clone(),
					                         Coredata::Internal(Commands::Prepare(tail.clone())))));
				program.push(head.clone());
			}
			&Sourcedata(ref source, Coredata::Symbol(ref string)) => {
				if let Some(number) = BigInt::parse_bytes(string.as_bytes(), 10) {
					env.result = Rc::new(Sourcedata(source.clone(), Coredata::Integer(number)));
				} else {
					let error = if let Some(value) = env.store.get(string) {
						if let Some(value) = value.last() {
							env.result = value.clone();
							None
						} else {
							if let &Some(ref source) = source {
								Some(format!["`{}' does exist but its stack is empty => {}",
								             string,
								             source])
							} else {
								Some(format!["`{}' does exist but its stack is empty", string])
							}
						}
					} else {
						if let &Some(ref source) = source {
							Some(format!["`{}' does not exist => {}", string, source])
						} else {
							Some(format!["`{}' does not exist", string])
						}
					};
					if let Some(error) = error {
						unwind_with_error_message(&error, &mut program, &mut env);
					}
				}
			}
			_ => {
				env.result = top.clone();
			}
		}
	}
	println!("{}", env.result);
	env
}

/// Initializes the environment with the standard library.
///
/// ```
/// extern crate teko;
/// let env: teko::data_structures::Env =
/// 	teko::interpret::initialize_environment_with_standard_library();
/// ```
pub fn initialize_environment_with_standard_library() -> Env {
	Env {
		store: create_builtin_library_table(),
		params: Vec::with_capacity(VEC_CAPACITY),
		result: Rc::new(Sourcedata(None, Coredata::Null)),
	}
}

/// Sets up a standard environment and evaluate the program.
///
/// Used to evaluate a program with the standard library and all builtins.
///
/// ```
/// extern crate teko;
/// extern crate num_traits;
/// use num_traits::cast::ToPrimitive;
/// fn main() {
/// 	let program = teko::parse::parse_string("(+ 1 2 4) (+ 1 2)").ok().unwrap();
/// 	let env = teko::interpret::interpret(program);
/// 	match env.result.1 {
/// 		teko::data_structures::Coredata::Integer(ref value) => {
/// 			assert_eq![value.to_i32().unwrap(), 3];
/// 		}
/// 		_ => {
/// 			panic!["Expected Integer but got a different data type"];
/// 		}
/// 	}
/// }
/// ```
pub fn interpret(program: Program) -> Env {
	let env = initialize_environment_with_standard_library();
	eval(program, env)
}

#[cfg(test)]
mod tests {
	use super::*;
	use parse::parse_file;
	#[test]
	fn test_interpreter() {
		let p = parse_file("input").ok().unwrap();
		interpret(p);
	}
}