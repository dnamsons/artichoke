use onig::{Regex, SearchOptions, Syntax};
use std::convert::TryFrom;
use std::hash::{Hash, Hasher};
use std::mem;
use std::rc::Rc;

use crate::convert::{FromMrb, RustBackedValue};
use crate::def::{rust_data_free, ClassLike, Define};
use crate::eval::MrbEval;
use crate::extn::core::error::{RubyException, RuntimeError, SyntaxError, TypeError};
use crate::sys;
use crate::value::Value;
use crate::{Mrb, MrbError};

pub mod enc;
pub mod opts;
pub mod syntax;

mod args;
pub mod case_compare;
pub mod casefold;
pub mod eql;
pub mod escape;
pub mod fixed_encoding;
pub mod hash;
pub mod initialize;
pub mod match_;
pub mod match_operator;
pub mod named_captures;
pub mod names;
pub mod options;
pub mod source;
pub mod to_s;
pub mod union;

pub fn init(interp: &Mrb) -> Result<(), MrbError> {
    interp.eval(include_str!("regexp.rb"))?;
    let regexp =
        interp
            .borrow_mut()
            .def_class::<Regexp>("Regexp", None, Some(rust_data_free::<Regexp>));
    regexp.borrow_mut().mrb_value_is_rust_backed(true);
    regexp.borrow_mut().add_method(
        "initialize",
        Regexp::initialize,
        sys::mrb_args_req_and_opt(1, 2),
    );
    regexp
        .borrow_mut()
        .add_self_method("compile", Regexp::compile, sys::mrb_args_rest());
    regexp
        .borrow_mut()
        .add_self_method("escape", Regexp::escape, sys::mrb_args_req(1));
    regexp
        .borrow_mut()
        .add_self_method("quote", Regexp::escape, sys::mrb_args_req(1));
    regexp
        .borrow_mut()
        .add_self_method("union", Regexp::union, sys::mrb_args_rest());
    regexp
        .borrow_mut()
        .add_method("==", Regexp::eql, sys::mrb_args_req(1));
    regexp
        .borrow_mut()
        .add_method("===", Regexp::case_compare, sys::mrb_args_req(1));
    regexp
        .borrow_mut()
        .add_method("=~", Regexp::match_operator, sys::mrb_args_req(1));
    regexp
        .borrow_mut()
        .add_method("casefold?", Regexp::casefold, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("eql?", Regexp::eql, sys::mrb_args_req(1));
    regexp.borrow_mut().add_method(
        "fixed_encoding?",
        Regexp::fixed_encoding,
        sys::mrb_args_none(),
    );
    regexp
        .borrow_mut()
        .add_method("hash", Regexp::hash, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("inspect", Regexp::inspect, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("match?", Regexp::is_match, sys::mrb_args_req_and_opt(1, 1));
    regexp
        .borrow_mut()
        .add_method("match", Regexp::match_, sys::mrb_args_req_and_opt(1, 1));
    regexp.borrow_mut().add_method(
        "named_captures",
        Regexp::named_captures,
        sys::mrb_args_none(),
    );
    regexp
        .borrow_mut()
        .add_method("names", Regexp::names, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("options", Regexp::options, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("source", Regexp::source, sys::mrb_args_none());
    regexp
        .borrow_mut()
        .add_method("to_s", Regexp::to_s, sys::mrb_args_none());
    regexp.borrow().define(&interp)?;
    // TODO: Add proper constant defs to class::Spec and undo this hack.
    interp.eval(format!(
        "class Regexp; IGNORECASE = {}; EXTENDED = {}; MULTILINE = {}; FIXEDENCODING = {}; NOENCODING = {}; end",
        Regexp::IGNORECASE,
        Regexp::EXTENDED,
        Regexp::MULTILINE,
        Regexp::FIXEDENCODING,
        Regexp::NOENCODING,
    ))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Regexp {
    literal_pattern: String,
    pattern: String,
    literal_options: opts::Options,
    options: opts::Options,
    encoding: enc::Encoding,
    pub regex: Rc<Regex>,
}

impl Default for Regexp {
    fn default() -> Self {
        Self {
            literal_pattern: String::default(),
            pattern: String::default(),
            literal_options: opts::Options::default(),
            options: opts::Options::default(),
            encoding: enc::Encoding::default(),
            regex: Rc::new(unsafe { mem::uninitialized::<Regex>() }),
        }
    }
}

impl Hash for Regexp {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.literal_pattern.hash(state);
        self.literal_options.hash(state);
    }
}

impl RustBackedValue for Regexp {
    fn new_obj_args(&self, interp: &Mrb) -> Vec<sys::mrb_value> {
        vec![
            Value::from_mrb(interp, self.literal_pattern.as_str()).inner(),
            Value::from_mrb(interp, self.literal_options.flags().bits()).inner(),
            Value::from_mrb(interp, self.encoding.flags()).inner(),
        ]
    }
}

impl Regexp {
    pub const IGNORECASE: i64 = 1;
    pub const EXTENDED: i64 = 2;
    pub const MULTILINE: i64 = 4;

    pub const ALL_REGEXP_OPTS: i64 = Self::IGNORECASE | Self::EXTENDED | Self::MULTILINE;

    pub const FIXEDENCODING: i64 = 16;
    pub const NOENCODING: i64 = 32;

    pub const ALL_ENCODING_OPTS: i64 = Self::FIXEDENCODING | Self::NOENCODING;

    pub fn new(
        literal_pattern: String,
        pattern: String,
        literal_options: opts::Options,
        options: opts::Options,
        encoding: enc::Encoding,
    ) -> Option<Self> {
        let regex = Rc::new(Regex::with_options(&pattern, options.flags(), Syntax::ruby()).ok()?);
        let regexp = Self {
            literal_pattern,
            pattern,
            literal_options,
            options,
            encoding,
            regex,
        };
        Some(regexp)
    }

    unsafe extern "C" fn initialize(
        mrb: *mut sys::mrb_state,
        slf: sys::mrb_value,
    ) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let result = initialize::Args::extract(&interp)
            .and_then(|args| initialize::method(&interp, args, slf));
        match result {
            Ok(value) => value.inner(),
            Err(initialize::Error::NoImplicitConversionToString) => {
                TypeError::raise(&interp, "no implicit conversion into String");
                unwrap_value_or_raise!(interp, Self::default().try_into_ruby(&interp, Some(slf)))
            }
            Err(initialize::Error::Syntax) => {
                SyntaxError::raise(&interp, "Failed to parse Regexp pattern");
                unwrap_value_or_raise!(interp, Self::default().try_into_ruby(&interp, Some(slf)))
            }
            Err(initialize::Error::Unicode) => {
                RuntimeError::raise(&interp, "Pattern is invalid UTF-8");
                unwrap_value_or_raise!(interp, Self::default().try_into_ruby(&interp, Some(slf)))
            }
            Err(initialize::Error::Fatal) => {
                RuntimeError::raise(&interp, "Fatal Regexp#initialize error");
                unwrap_value_or_raise!(interp, Self::default().try_into_ruby(&interp, Some(slf)))
            }
        }
    }

    unsafe extern "C" fn compile(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let args = mem::uninitialized::<*const sys::mrb_value>();
        let count = mem::uninitialized::<sys::mrb_int>();
        sys::mrb_get_args(mrb, b"*\0".as_ptr() as *const i8, &args, &count);
        sys::mrb_obj_new(mrb, sys::mrb_sys_class_ptr(slf), count, args)
    }

    unsafe extern "C" fn escape(mrb: *mut sys::mrb_state, _slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let result = escape::Args::extract(&interp).and_then(|args| escape::method(&interp, &args));
        match result {
            Ok(result) => result.inner(),
            Err(escape::Error::NoImplicitConversionToString) => {
                TypeError::raise(&interp, "no implicit conversion into String")
            }
            Err(escape::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp::escape error"),
        }
    }

    unsafe extern "C" fn union(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let args = union::Args::extract(&interp);
        let result = union::method(&interp, args, slf);
        match result {
            Ok(result) => result.inner(),
            Err(union::Error::NoImplicitConversionToString) => {
                TypeError::raise(&interp, "no implicit conversion into String")
            }
            Err(union::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp::union error"),
        }
    }

    unsafe extern "C" fn is_match(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let args = unwrap_or_raise!(
            interp,
            args::Match::extract(&interp),
            sys::mrb_sys_nil_value()
        );

        let data = unwrap_or_raise!(
            interp,
            Self::try_from_ruby(&interp, &Value::new(&interp, slf)),
            sys::mrb_sys_nil_value()
        );
        let string = match args.string {
            Ok(Some(ref string)) => string.to_owned(),
            Err(_) => return TypeError::raise(&interp, "No implicit conversion into String"),
            _ => return sys::mrb_sys_nil_value(),
        };

        let pos = args.pos.unwrap_or_default();
        let pos = if pos < 0 {
            let strlen = i64::try_from(string.len()).unwrap_or_default();
            let pos = strlen + pos;
            if pos < 0 {
                return sys::mrb_sys_nil_value();
            }
            usize::try_from(pos).expect("positive i64 must be usize")
        } else {
            usize::try_from(pos).expect("positive i64 must be usize")
        };
        // onig will panic if pos is beyond the end of string
        if pos > string.len() {
            return Value::from_mrb(&interp, false).inner();
        }
        let is_match = data.borrow().regex.search_with_options(
            string.as_str(),
            pos,
            string.len(),
            SearchOptions::SEARCH_OPTION_NONE,
            None,
        );
        Value::from_mrb(&interp, is_match.is_some()).inner()
    }

    unsafe extern "C" fn match_(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        let result =
            match_::Args::extract(&interp).and_then(|args| match_::method(&interp, args, &value));
        match result {
            Ok(result) => result.inner(),
            Err(match_::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#match error"),
            Err(match_::Error::PosType) => {
                TypeError::raise(&interp, "No implicit conversion into Integer")
            }
            Err(match_::Error::StringType) => {
                TypeError::raise(&interp, "No implicit conversion into String")
            }
        }
    }

    unsafe extern "C" fn eql(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let args = eql::Args::extract(&interp);
        let value = Value::new(&interp, slf);
        match eql::method(&interp, args, &value) {
            Ok(result) => result.inner(),
            Err(eql::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#== error"),
        }
    }

    unsafe extern "C" fn case_compare(
        mrb: *mut sys::mrb_state,
        slf: sys::mrb_value,
    ) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        let result = case_compare::Args::extract(&interp)
            .and_then(|args| case_compare::method(&interp, args, &value));
        match result {
            Ok(result) => result.inner(),
            Err(case_compare::Error::NoImplicitConversionToString) => {
                Value::from_mrb(&interp, false).inner()
            }
            Err(case_compare::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#=== error")
            }
        }
    }

    unsafe extern "C" fn match_operator(
        mrb: *mut sys::mrb_state,
        slf: sys::mrb_value,
    ) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        let result = match_operator::Args::extract(&interp)
            .and_then(|args| match_operator::method(&interp, args, &value));
        match result {
            Ok(result) => result.inner(),
            Err(match_operator::Error::NoImplicitConversionToString) => {
                Value::from_mrb(&interp, false).inner()
            }
            Err(match_operator::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#=== error")
            }
        }
    }

    unsafe extern "C" fn casefold(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match casefold::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(casefold::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#casefold? error")
            }
        }
    }

    unsafe extern "C" fn fixed_encoding(
        mrb: *mut sys::mrb_state,
        slf: sys::mrb_value,
    ) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match fixed_encoding::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(fixed_encoding::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#fixed_encoding? error")
            }
        }
    }

    unsafe extern "C" fn hash(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match hash::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(hash::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#hash error"),
        }
    }

    unsafe extern "C" fn inspect(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let regexp = unwrap_or_raise!(
            interp,
            Self::try_from_ruby(&interp, &Value::new(&interp, slf)),
            sys::mrb_sys_nil_value()
        );
        let s = format!(
            "/{}/{}{}",
            regexp.borrow().literal_pattern.as_str().replace("/", r"\/"),
            regexp.borrow().literal_options.modifier_string(),
            regexp.borrow().encoding.string()
        );
        Value::from_mrb(&interp, s).inner()
    }

    unsafe extern "C" fn named_captures(
        mrb: *mut sys::mrb_state,
        slf: sys::mrb_value,
    ) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match named_captures::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(named_captures::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#named_captures error")
            }
        }
    }

    unsafe extern "C" fn names(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match names::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(names::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#names error"),
        }
    }

    unsafe extern "C" fn options(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match options::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(options::Error::Fatal) => {
                RuntimeError::raise(&interp, "fatal Regexp#options error")
            }
        }
    }

    unsafe extern "C" fn source(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match source::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(source::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#source error"),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    unsafe extern "C" fn to_s(mrb: *mut sys::mrb_state, slf: sys::mrb_value) -> sys::mrb_value {
        let interp = interpreter_or_raise!(mrb);
        let value = Value::new(&interp, slf);
        match to_s::method(&interp, &value) {
            Ok(result) => result.inner(),
            Err(to_s::Error::Fatal) => RuntimeError::raise(&interp, "fatal Regexp#to_s error"),
        }
    }
}
