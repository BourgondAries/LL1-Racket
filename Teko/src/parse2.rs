use std::fs::File;
use std::io::Read;
use std::rc::Rc;
use super::VEC_CAPACITY;

use interpret2::Data;
use interpret2::Source;

#[derive(Debug)]
pub struct ParseState {
	current_read_position:         Source,
	start_of_current_lexeme:       Source,
	unmatched_opening_parentheses: Vec<Source>,
	token: String,
	stack: Vec<Rc<Data>>,
	error: Option<String>,
}

impl Default for ParseState {
	fn default() -> ParseState {
		ParseState {
			current_read_position:         Source::default(),
			start_of_current_lexeme:       Source::default(),
			unmatched_opening_parentheses: Vec::with_capacity(VEC_CAPACITY),
			token: String::from(""),
			stack: Vec::with_capacity(VEC_CAPACITY),
			error: None,
		}
	}
}

impl ParseState {
	fn from_file(filename: &str) -> ParseState {
		let mut state = ParseState::default();
		state.current_read_position = Source {
			line:   1,
			column: 1,
			source: filename.into(),
		};
		state
	}
}

pub fn parse_file(filename: &str) -> Result<Vec<Rc<Data>>, ParseState> {
	let mut file = File::open(filename).ok().unwrap();
	let mut contents = String::new();
	file.read_to_string(&mut contents).ok();
	parse_string_with_state(&contents, ParseState::from_file(filename))
}

////////////////////////////////////////////////////////////

pub fn parse_string(string: &str) -> Result<Vec<Rc<Data>>, ParseState> {
	let mut state = ParseState::default();
	parse_string_with_state(string, state)
}

////////////////////////////////////////////////////////////

fn parse_string_with_state(string: &str, mut state: ParseState) -> Result<Vec<Rc<Data>>, ParseState> {
	for character in string.chars() {
		parse_character(character, &mut state);
		if state.error.is_some() {
			break;
		}
	}
	finish_parsing_characters(state)
}

////////////////////////////////////////////////////////////

pub fn finish_parsing_characters(mut state: ParseState) -> Result<Vec<Rc<Data>>, ParseState> {
	whitespace(&mut state);
	if ! state.unmatched_opening_parentheses.is_empty() {
		set_error(&mut state, "Unmatched opening parenthesis");
		Err(state)
	} else if state.error.is_some() {
		Err(state)
	} else {
		Ok(state.stack)
	}
}

pub fn parse_character(character: char, state: &mut ParseState) {
	parse_internal(character, state);
	count_characters_and_lines(character, state);
}

////////////////////////////////////////////////////////////
// Internal                                               //
////////////////////////////////////////////////////////////

fn count_characters_and_lines(character: char, state: &mut ParseState) {
	if character == '\n' {
		state.current_read_position.line   += 1;
		state.current_read_position.column =  1;
	} else {
		state.current_read_position.column += 1;
	}
}

fn parse_internal(character: char, state: &mut ParseState) {
	if character.is_whitespace() {
		whitespace(state);
	} else if character == '(' {
		left_parenthesis(state);
	} else if character == ')' {
		right_parenthesis(state);
	} else {
		otherwise(character, state);
	}
}

////////////////////////////////////////////////////////////

fn whitespace(state: &mut ParseState) {
	move_token_to_stack(state);
}

fn left_parenthesis(state: &mut ParseState) {
	move_token_to_stack(state);
	copy_current_read_position_to_unmatched_opening_parentheses(state);
	state.stack.push(Rc::new(Data::Internal(state.current_read_position.clone())));
}

fn right_parenthesis(state: &mut ParseState) {
	move_token_to_stack(state);
	pop_previous_opening_parenthesis(state);
	let mut active = Rc::new(Data::Null(state.current_read_position.clone()));
	let mut source = Source::default();
	while let Some(top) = state.stack.pop() {
		match &*top {
			&Data::Internal(ref pair_source, ..) => {
				source = pair_source.clone();
				break;
			}
			_ => {
				active = Rc::new(Data::Pair(top.clone().get_source(), top.clone(), active));
			},
		}
	}
	Rc::get_mut(&mut active).expect("There are no other references to the active set").set_source(source);
	state.stack.push(active);
}

fn otherwise(character: char, state: &mut ParseState) {
	if state.token.is_empty() {
		state.start_of_current_lexeme = state.current_read_position.clone();
	}
	state.token.push(character);
}

////////////////////////////////////////////////////////////

fn move_token_to_stack(state: &mut ParseState) {
	if ! state.token.is_empty() {
		state.stack.push(Rc::new(Data::String(state.start_of_current_lexeme.clone(), state.token.clone())));
		clear_token(state);
	}
}

fn clear_token(state: &mut ParseState) {
	state.token.clear();
}

fn set_error(state: &mut ParseState, message: &str) {
	state.error = Some(String::from(message));
}

fn copy_current_read_position_to_unmatched_opening_parentheses(state: &mut ParseState) {
	state.unmatched_opening_parentheses.push(state.current_read_position.clone());
}

fn pop_previous_opening_parenthesis(state: &mut ParseState) {
	if ! state.unmatched_opening_parentheses.pop().is_some() {
		set_error(state, "Unmatched closing parenthesis");
	}
}

////////////////////////////////////////////////////////////
// Tests                                                  //
////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
	use super::*;
	macro_rules! assert_oks {
		( $f:expr, $( $x:expr ),*, ) => { assert_oks![$f, $( $x ),*]; };
		( $f:expr, $( $x:expr ),* ) => { { $( assert![$f($x).is_ok()]; )* } };
	}
	macro_rules! assert_errs {
		( $f:expr, $( $x:expr ),*, ) => { assert_errs![$f, $( $x ),*]; };
		( $f:expr, $( $x:expr ),* ) => { { $( assert![$f($x).is_err()]; )* } };
	}
	#[test]
	fn assert_expressions_ok() {
		return;
		assert_oks![
			parse_string,
			"", " ", "  ", "[", "]", "{", "}", ".", ",", "'", "\"",
			"", " ", "  ", "[", "]>", "<{", "}|", ".^", ",-", "'", "\"",
			"()", " ()", "() ", " () ", " ( ) ",
			"test", "(test)", " (test)", "(test) ", " (test) ",
			"(test1 (test2))",
			"(test1 (test2 test3 test4) test5) test6",
		];
	}

	#[test]
	fn assert_expressions_err() {
		return;
		assert_errs![
			parse_string,
			"(",
			")",
			"(test",
			"test)",
			"(test1 (test2)"
		];
	}
}