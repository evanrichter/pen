use super::TypeError;
use crate::types::*;
use fnv::FnvHashMap;

pub fn resolve(reference: &Reference, types: &FnvHashMap<String, Type>) -> Result<Type, TypeError> {
    resolve_type(&reference.clone().into(), types)
}

fn resolve_type(type_: &Type, types: &FnvHashMap<String, Type>) -> Result<Type, TypeError> {
    Ok(match type_ {
        Type::Reference(reference) => resolve_type(
            types
                .get(reference.name())
                .ok_or_else(|| TypeError::TypeNotFound(reference.clone()))?,
            types,
        )?,
        _ => type_.clone(),
    })
}
