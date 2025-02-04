use crate::extn::prelude::*;

pub fn init(interp: &mut Artichoke) -> InitializeResult<()> {
    if interp.0.borrow().class_spec::<Range>().is_some() {
        return Ok(());
    }
    let spec = class::Spec::new("Range", None, None)?;
    interp.0.borrow_mut().def_class::<Range>(spec);
    let _ = interp.eval(&include_bytes!("range.rb")[..])?;
    trace!("Patched Range onto interpreter");
    Ok(())
}

#[derive(Debug)]
pub struct Range;
