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

use num::bigint::ToBigInt;
use num::bigint::BigInt;

use data_structures::{Boolean, Commands, Env, Program, Sourcedata, Coredata, Statement, Macro,
                      Function};

/// Macro::Builtin types are called with env.result as input, so (macro a b c) has
/// env.result = (a b c).
/// Macro::Library types are called by binding input to the macro's input variable.
/// Function::Builtin types are called with env.params containing evaluated parameters.
/// Function::Library types are called with env.params containing evaluated parameters,
/// bound to parameters.

/// Optimizes tail calls by seeing if the current `params` can be merged with the top of the stack.
///
/// If the top of the stack contains `Commands::Deparameterize`, then the variables to be popped
/// are merged into that [top] object. This is all that's needed to optimize tail calls.
fn optimize_tail_call(program: &mut Program, env: &mut Env, params: &Vec<String>) -> Vec<String> {
	// TODO break this function up, it does 2 things, merge a list of strings and pop stuff from
	// the stack
	if let Some(top) = program.pop() {
		let mut to_add = vec![];
		match top.1 {
			Coredata::Internal(Commands::Deparameterize(ref content)) => {
				for parameter in params {
					if content.contains(parameter) {
						if let Some(ref mut entry) = env.store.get_mut(parameter) {
							if let Some(_) = entry.pop() {
								// OK
							} else {
								panic!["Unable to pop entry"];
							}
						} else {
							panic!["Unable to perform TCO"];
						}
					} else {
						to_add.push(parameter.clone());
					}
				}
				to_add.extend(content.iter().cloned());
				to_add
			}
			_ => {
				program.push(top.clone());
				params.clone()
			}
		}
	} else {
		params.clone()
	}
}

/// Quote elements
///
/// A builtin macro always stores the tail of the invocation inside `env.result`, so this macro is
/// empty; it doesn't need to do anything.
fn quote(_: &Statement, _: &mut Program, _: &mut Env) {
	println!["Created quoted list"];
}

fn eval_expose(_: &Statement, program: &mut Program, env: &mut Env) {
	let error: Option<_> = if let Some(args) = env.params.last() {
		if args.len() != 1 {
			Some("eval: arity mismatch")
		} else {
			if let Some(arg) = args.first() {
				program.push(arg.clone());
				None
			} else {
				Some("eval: arity mismatch")
			}
		}
	} else {
		Some("eval: parameter stack empty")
	};
	if let Some(error) = error {
		unwind_with_error_message(error, program, env);
	}
}

/// Create a string
///
/// Creates a string from the given symbols by inserting single spaces inbetween each symbol.
/// TODO: Allow subexpressions; implement string interpolation and non-printable
/// character insertion.
fn string(_: &Statement, _: &mut Program, env: &mut Env) {
	let vec = collect_pair_into_vec_string(&env.result);
	env.result = Rc::new(Sourcedata(None, Coredata::String(vec.join(" "))));
	println!["Created string"];
}

fn wind(_: &Statement, program: &mut Program, env: &mut Env) {
	println!["Wind macro"];
	let args = env.result.clone();
	let code = collect_pair_into_vec(&args);
	program.push(Rc::new(Sourcedata(None, Coredata::Internal(Commands::Wind))));
	program.extend(code.iter().cloned());
}

fn unwind(_: &Statement, program: &mut Program, env: &mut Env) {
	println!["Unwind macro"];
	env.result = env.params.last().unwrap().last().unwrap().clone();
	while let Some(top) = program.pop() {
		match top.1 {
			Coredata::Internal(Commands::Deparameterize(ref arguments)) => {
				pop_parameters(&top, program, env, arguments);
			}
			Coredata::Internal(Commands::Call(..)) => {
				env.params.pop();
			}
			Coredata::Internal(Commands::Wind) => {
				break;
			}
			_ => {}
		}
	}
}

fn make_unwind() -> Statement {
	let sub = Rc::new(Sourcedata(None, Coredata::Function(Function::Builtin(unwind))));
	Rc::new(Sourcedata(None, Coredata::Internal(Commands::Call(sub))))
}

fn unwind_with_error_message(string: &str, program: &mut Program, env: &mut Env) {
	let sub = Rc::new(Sourcedata(None, Coredata::String(string.into())));
	env.params.push(vec![Rc::new(Sourcedata(None, Coredata::Error(sub)))]);
	program.push(make_unwind());
}

fn error(_: &Statement, program: &mut Program, env: &mut Env) {
	if let Some(args) = env.params.last() {
		if args.len() >= 2 {
			env.result =
				Rc::new(Sourcedata(None,
				                   Coredata::Error(Rc::new(Sourcedata(None,
				                                                      Coredata::String("Arity \
				                                                                        mismatch; \
				                                                                        Too \
				                                                                        many \
				                                                                        arguments \
				                                                                        to error"
					                                                      .into()))))));
			program.push(make_unwind());
		} else {
			if let Some(arg) = args.first() {
				env.result = Rc::new(Sourcedata(None, Coredata::Error(arg.clone())));
			} else {
				env.result =
					Rc::new(Sourcedata(None,
					                   Coredata::Error(Rc::new(Sourcedata(None, Coredata::Null)))));
			}
		}
	} else {
		panic!["The parameter list does not contain a list; this is an internal error that \
		        should not happen"];
	}
}

fn not(_: &Statement, program: &mut Program, env: &mut Env) {
	let args = env.params.last().expect("Should exist by virtue of functions");
	if args.len() != 1 {
		program.push(make_unwind());
		println!["Should have a single arg"];
	} else {
		if let Coredata::Null = args.first().unwrap().1 {
			env.result = Rc::new(Sourcedata(None, Coredata::Symbol("true".into())));
		} else {
			env.result = Rc::new(Sourcedata(None, Coredata::Null));
		}
	}
}

fn head(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.params.last().expect("Should exist by virtue of functions");
	if args.len() != 1 {
		panic!("should have only a single arg");
	} else {
		env.result = args.first().unwrap().head().clone();
	}
}

fn tail(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.params.last().expect("Should exist by virtue of functions");
	if args.len() != 1 {
		panic!("should have only a single arg");
	} else {
		env.result = args.first().unwrap().tail().clone();
	}
	println!["Took tail"];
}

fn pair(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.params.last().expect("Should exist by virtue of functions");
	if args.len() != 2 {
		panic!("should have two args");
	} else {
		env.result = Rc::new(Sourcedata(None,
		                                Coredata::Pair(args.first().unwrap().clone(),
		                                               args.get(1).unwrap().clone())));
	}
	println!["Took tail"];
}

fn make_macro(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.result.clone();
	let params = match args.head().1 {
		Coredata::Symbol(ref string) => string.clone(),
		_ => {
			panic!("Wrong use of macro");
		}
	};
	let code = collect_pair_into_vec(&args.tail());
	env.result = Rc::new(Sourcedata(None, Coredata::Macro(Macro::Library(params, code))));
	println!["Created macro object"];
}

fn function(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.result.clone();
	let params = collect_pair_into_vec_string(&args.head());
	let code = collect_pair_into_vec(&args.tail());
	env.result = Rc::new(Sourcedata(None, Coredata::Function(Function::Library(params, code))));
	println!["Created function object"];
}

fn set(_: &Statement, _: &mut Program, _: &mut Env) {
	unimplemented!();
}

fn define(_: &Statement, program: &mut Program, env: &mut Env) {
	{
		let arguments = env.result.clone();
		let sub = Rc::new(Sourcedata(None, Coredata::Function(Function::Builtin(define_internal))));
		program.push(Rc::new(Sourcedata(None, Coredata::Internal(Commands::Call(sub)))));
		program.push(Rc::new(Sourcedata(None, Coredata::Internal(Commands::Parameterize))));
		match arguments.tail().1 {
			Coredata::Pair(ref heado, _) => {
				program.push(heado.clone());
			}
			_ => {
				panic!{"it cant be"};
			}
		}
		program.push(Rc::new(Sourcedata(None, Coredata::Internal(Commands::Parameterize))));
		match arguments.head().1 {
			Coredata::Symbol(ref string) => {
				program.push(Rc::new(Sourcedata(None, Coredata::String(string.clone()))));
			}
			_ => {
				panic!("Define did not get a symbol!");
			}
		}
	}
	env.params.push(vec![]);
}

fn if_conditional(_: &Statement, program: &mut Program, env: &mut Env) {
	let arguments = env.result.clone();
	program.push(Rc::new(Sourcedata(None,
	                                Coredata::Internal(Commands::If(arguments.tail().head(),
	                                                                arguments.tail()
		                                                                .tail()
		                                                                .head())))));
	program.push(arguments.head());
}

fn define_internal(_: &Statement, _: &mut Program, env: &mut Env) {
	let args = env.params.last().expect("Must be defined by previous macro");
	match args[0].1 {
		Coredata::String(ref string) => {
			env.store.insert(string.clone(), vec![args[1].clone()]);
		}
		_ => {
			unimplemented!();
		}
	}
}

fn sleep(_: &Statement, _: &mut Program, env: &mut Env) {
	use std::{thread, time};
	use num::ToPrimitive;
	println!["Sleep func"];
	let arguments = env.params
		.last()
		.expect("The state machine should ensure this exists")
		.first()
		.expect("Srs guys");
	match arguments.1 {
		Coredata::Integer(ref value) => {
			thread::sleep(time::Duration::from_millis(value.to_u64()
				.expect("Handling non numbers not implemented yet")));
		}
		_ => {}
	}
}

fn geq(_: &Statement, _: &mut Program, env: &mut Env) {
	let arguments = env.params.last().expect("The state machine should ensure this exists");
	let mut last = None;
	let mut result = Rc::new(Sourcedata(None, Coredata::Boolean(Boolean::True)));
	for argument in arguments.iter() {
		match &**argument {
			&Sourcedata(_, Coredata::Complex(_)) => {
				unimplemented![];
			}
			&Sourcedata(_, Coredata::Integer(ref integer)) => {
				if let Some(previous) = last {
					if previous >= integer {
						// Do nothing
					} else {
						result = Rc::new(Sourcedata(None, Coredata::Null));
						break;
					}
					last = Some(integer);
				} else {
					last = Some(integer);
				}
			}
			&Sourcedata(_, Coredata::Rational(_)) => {
				unimplemented![];
			}
			ref a => {
				println!["{}", a];
				unimplemented![];
			}
		}
	}
	env.result = result;
}

fn plus(_: &Statement, _: &mut Program, env: &mut Env) {
	let arguments = env.params.last().expect("The state machine should ensure this exists");
	let mut sum = 0.to_bigint().expect("Constant zero should always be parsed correctly");
	for argument in arguments.iter() {
		match &**argument {
			&Sourcedata(_, Coredata::Complex(_)) => {
				unimplemented![];
			}
			&Sourcedata(_, Coredata::Integer(ref integer)) => {
				sum = sum + integer;
			}
			&Sourcedata(_, Coredata::Rational(_)) => {
				unimplemented![];
			}
			ref a => {
				println!["{}", a];
				unimplemented![];
			}
		}
	}
	env.result = Rc::new(Sourcedata(None, Coredata::Integer(sum)));
}

fn minus(_: &Statement, _: &mut Program, env: &mut Env) {
	let arguments = env.params.last().expect("The state machine should ensure this exists");
	let mut sum = 0.to_bigint().expect("Constant zero should always be parsed correctly");
	if arguments.len() == 1 {
		for argument in arguments.iter() {
			match &**argument {
				&Sourcedata(_, Coredata::Complex(_)) => {
					unimplemented![];
				}
				&Sourcedata(_, Coredata::Integer(ref integer)) => {
					sum = sum - integer;
				}
				&Sourcedata(_, Coredata::Rational(_)) => {
					unimplemented![];
				}
				_ => {
					unimplemented![];
				}
			}
		}
	} else if arguments.len() > 1 {
		let mut first = true;
		for argument in arguments.iter() {
			match &**argument {
				&Sourcedata(_, Coredata::Complex(_)) => {
					unimplemented![];
				}
				&Sourcedata(_, Coredata::Integer(ref integer)) => {
					if first {
						sum = integer.clone();
					} else {
						sum = sum - integer;
					}
				}
				&Sourcedata(_, Coredata::Rational(_)) => {
					unimplemented![];
				}
				_ => {
					unimplemented![];
				}
			}
			first = false;
		}
	}
	env.result = Rc::new(Sourcedata(None, Coredata::Integer(sum)));
}

fn multiply(_: &Statement, _: &mut Program, env: &mut Env) {
	let arguments = env.params.last().expect("The state machine should ensure this exists");
	let mut sum = 1.to_bigint().expect("Constant zero should always be parsed correctly");
	for argument in arguments.iter() {
		match &**argument {
			&Sourcedata(_, Coredata::Complex(_)) => {
				unimplemented![];
			}
			&Sourcedata(_, Coredata::Integer(ref integer)) => {
				sum = sum * integer;
			}
			&Sourcedata(_, Coredata::Rational(_)) => {
				unimplemented![];
			}
			_ => {
				unimplemented![];
			}
		}
	}
	println!["plus: {}", sum];
	env.result = Rc::new(Sourcedata(None, Coredata::Integer(sum)));
}

fn divide(_: &Statement, _: &mut Program, env: &mut Env) {
	let arguments = env.params.last().expect("The state machine should ensure this exists");
	let mut sum = 1.to_bigint().expect("Constant zero should always be parsed correctly");
	if arguments.len() == 1 {
		for argument in arguments.iter() {
			match &**argument {
				&Sourcedata(_, Coredata::Complex(_)) => {
					unimplemented![];
				}
				&Sourcedata(_, Coredata::Integer(ref integer)) => {
					sum = sum / integer;
				}
				&Sourcedata(_, Coredata::Rational(_)) => {
					unimplemented![];
				}
				_ => {
					unimplemented![];
				}
			}
		}
	} else if arguments.len() > 1 {
		let mut first = true;
		for argument in arguments.iter() {
			match &**argument {
				&Sourcedata(_, Coredata::Complex(_)) => {
					unimplemented![];
				}
				&Sourcedata(_, Coredata::Integer(ref integer)) => {
					if first {
						sum = integer.clone();
					} else {
						sum = sum / integer;
					}
				}
				&Sourcedata(_, Coredata::Rational(_)) => {
					unimplemented![];
				}
				_ => {
					unimplemented![];
				}
			}
			first = false;
		}
	} else {
		// Arity mismatch
		unimplemented!();
	}
	println!["plus: {}", sum];
	env.result = Rc::new(Sourcedata(None, Coredata::Integer(sum)));
}

fn collect_pair_into_vec_string(data: &Rc<Sourcedata>) -> Vec<String> {
	let data = collect_pair_into_vec(data);
	let mut ret = vec![];
	for i in data.iter() {
		match &**i {
			&Sourcedata(_, Coredata::Symbol(ref string)) => {
				ret.push(string.clone());
			}
			_ => {
				panic!{"Not a symbol"};
			}
		}
	}
	ret.reverse();
	ret
}

fn collect_pair_into_vec(data: &Rc<Sourcedata>) -> Vec<Rc<Sourcedata>> {
	let mut to_return = vec![];
	let mut current = data.clone();
	loop {
		current = if let &Sourcedata(_, Coredata::Pair(ref head, ref tail)) = &*current {
			to_return.push(head.clone());
			tail.clone()
		} else {
			break;
		}
	}
	to_return.reverse();
	to_return
}

fn pop_parameters(_: &Statement, _: &mut Program, env: &mut Env, args: &Vec<String>) {
	for arg in args {
		print!["{}, ", arg];
		env.store.get_mut(arg).expect("Should exist in the argument store!").pop();
		if env.store.get(arg).unwrap().is_empty() {
			env.store.remove(arg);
		}
	}
}

macro_rules! construct_builtins {
	($($t:tt : $e:expr => $i:ident),*,) => {
		construct_builtins![$($t : $e => $i),*]
	};
	($($t:tt : $e:expr => $i:ident),*) => {
		[
			$(
				($e.into(), vec![Rc::new(Sourcedata(None, Coredata::$t($t::Builtin($i))))])
			),*
		].iter().cloned().collect()
	};
}

/// Initializes the environment with the standard library
///
/// ```
/// extern crate teko;
/// let env: teko::data_structures::Env =
/// 	teko::interpret::initialize_environment_with_standard_library();
/// ```
pub fn initialize_environment_with_standard_library() -> Env {
	Env {
		store: construct_builtins! {
			// TODO: could be made even shorter by creating one space for functions and another for
			// macros, or too much context to be readable?
			Function : "+" => plus,
			Function : "-" => minus,
			Function : "*" => multiply,
			Function : "/" => divide,
			Function : ">=" => geq,
			Function : "not" => not,
			Function : "error" => error,
			Function : "head" => head,
			Function : "tail" => tail,
			Function : "pair" => pair,
			Function : "sleep" => sleep,
			Function : "unwind" => unwind,
			Function : "eval" => eval_expose,
			Macro : "'" => quote,
			Macro : "\"" => string,
			Macro : "if" => if_conditional,
			Macro : "set!" => set,
			Macro : "wind" => wind,
			Macro : "define" => define,
			Macro : "fn" => function,
			Macro : "mo" => make_macro,
		},
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
	program.reverse(); // TODO: Do this in the parser instead, doesn't fit in here.
	let mut coun = 0;
	while let Some(top) = program.pop() {
		println!["{}", top];
		coun += 1;
		match &*top {
			&Sourcedata(_, Coredata::Pair(ref head, ref tail)) => {
				program.push(Rc::new(Sourcedata(tail.0.clone(),
					                         Coredata::Internal(Commands::Prepare(tail.clone())))));
				program.push(head.clone());
			}
			&Sourcedata(_, Coredata::Internal(Commands::Wind)) => {
				// Do nothing
			}
			&Sourcedata(_, Coredata::Internal(Commands::Parameterize)) => {
				let succeeded = if let Some(ref mut last) = env.params.last_mut() {
					last.push(env.result.clone());
					false
				} else {
					true
				};
				if succeeded {
					unwind_with_error_message("Error during parameterization: the parameter \
					                           stack is nonexistent",
					                          &mut program,
					                          &mut env);
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Prepare(ref arguments))) => {
				match &*env.result.clone() {
					&Sourcedata(_, Coredata::Function(..)) => {
						env.params.push(vec![]);
						program.push(Rc::new(Sourcedata(env.result.0.clone(),
						                                Coredata::Internal(Commands::Call(env.result
							                                .clone())))));
						for argument in collect_pair_into_vec(arguments).iter() {
							program.push(Rc::new(Sourcedata(None, Coredata::Internal(Commands::Parameterize))));
							program.push(argument.clone());
						}
					}
					&Sourcedata(_, Coredata::Macro(Macro::Builtin(ref transfer))) => {
						env.result = arguments.clone();
						transfer(&top, &mut program, &mut env);
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
						let next = Rc::new(Sourcedata(None, deparam));
						program.push(next);
						program.extend(code.iter().cloned());
					}
					_ => {
						unwind_with_error_message("Error during prepare routine: element not \
						                           callable",
						                          &mut program,
						                          &mut env);
					}
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Evaluate)) => {
				program.push(env.result.clone());
			}
			&Sourcedata(_, Coredata::Internal(Commands::Call(ref statement))) => {
				match &**statement {
					&Sourcedata(_, Coredata::Function(Function::Builtin(ref transfer))) => {
						transfer(&top, &mut program, &mut env);
						if let Some(_) = env.params.pop() {
							// Do nothing
						} else {
							unwind_with_error_message("during builtin function call: parameter \
							                           stack not poppable",
							                          &mut program,
							                          &mut env);
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
								let next = Rc::new(Sourcedata(None, Coredata::Internal(cmd)));
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
			&Sourcedata(_, Coredata::Internal(Commands::If(ref first, ref second))) => {
				if let Coredata::Null = env.result.1 {
					program.push(second.clone());
				} else {
					program.push(first.clone());
				}
			}
			&Sourcedata(_, Coredata::Internal(Commands::Deparameterize(ref arguments))) => {
				pop_parameters(&top, &mut program, &mut env, arguments);
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
								Some(format!["{} => variable `{}' does exist but its stack is \
								              empty",
								             source,
								             string])
							} else {
								Some(format!["_ variable `{}' does exist but its stack is empty",
								             string])
							}
						}
					} else {
						if let &Some(ref source) = source {
							Some(format!["{} => variable `{}' does not exist", source, string])
						} else {
							Some(format!["variable `{}' does not exist", string])
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
	println!["{}", coun];
	env
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
