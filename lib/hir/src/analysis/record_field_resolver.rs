use super::{type_canonicalizer, AnalysisError};
use crate::types::*;
use fnv::FnvHashMap;

pub fn resolve<'a>(
    type_: &Type,
    types: &FnvHashMap<String, Type>,
    records: &'a FnvHashMap<String, Vec<RecordField>>,
) -> Result<&'a [RecordField], AnalysisError> {
    resolve_record(
        &type_canonicalizer::canonicalize_record(type_, types)?
            .ok_or_else(|| AnalysisError::RecordExpected(type_.clone()))?,
        records,
    )
}

pub fn resolve_record<'a>(
    record: &Record,
    records: &'a FnvHashMap<String, Vec<RecordField>>,
) -> Result<&'a [RecordField], AnalysisError> {
    Ok(records
        .get(record.name())
        .ok_or_else(|| AnalysisError::RecordNotFound(record.clone()))?)
}
