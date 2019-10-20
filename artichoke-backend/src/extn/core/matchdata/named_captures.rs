//! [`MatchData#named_captures`](https://ruby-doc.org/core-2.6.3/MatchData.html#method-i-named_captures)

use std::collections::HashMap;
use std::convert::TryFrom;

use crate::convert::{Convert, RustBackedValue};
use crate::extn::core::exception::{Fatal, RubyException};
use crate::extn::core::matchdata::MatchData;
use crate::extn::core::regexp::Backend;
use crate::value::Value;
use crate::Artichoke;

pub fn method(interp: &Artichoke, value: &Value) -> Result<Value, Box<dyn RubyException>> {
    let data = unsafe { MatchData::try_from_ruby(interp, value) }.map_err(|_| {
        Fatal::new(
            interp,
            "Unable to extract Rust MatchData from Ruby MatchData receiver",
        )
    })?;
    let borrow = data.borrow();
    let match_against = &borrow.string[borrow.region.start..borrow.region.end];
    let mut map = HashMap::default();
    let regex = (*borrow.regexp.regex)
        .as_ref()
        .ok_or_else(|| Fatal::new(interp, "Uninitalized Regexp"))?;
    match regex {
        Backend::Onig(regex) => {
            if let Some(captures) = regex.captures(match_against) {
                regex.foreach_name(|group, group_indexes| {
                    'name: for index in group_indexes.iter().rev() {
                        let index = usize::try_from(*index).unwrap_or_default();
                        if let Some(capture) = captures.at(index) {
                            map.insert(group.to_owned(), Some(capture.to_owned()));
                            break 'name;
                        }
                        map.insert(group.to_owned(), None);
                    }
                    true
                });
            } else {
                return Ok(interp.convert(None::<Value>));
            }
        }
        Backend::Rust(_) => unimplemented!("Rust-backed Regexp"),
    };
    Ok(interp.convert(map))
}
