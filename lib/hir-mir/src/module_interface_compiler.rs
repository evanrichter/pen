use super::{error::CompileError, type_extractor};
use hir::ir;

pub fn compile(module: &ir::Module) -> Result<interface::Module, CompileError> {
    Ok(interface::Module::new(
        module
            .type_definitions()
            .iter()
            .map(|definition| {
                interface::TypeDefinition::new(
                    definition.name(),
                    definition.original_name(),
                    definition.elements().to_vec(),
                    definition.is_open(),
                    definition.is_public() && !definition.is_external(),
                    definition.position().clone(),
                )
            })
            .collect(),
        module
            .type_aliases()
            .iter()
            .map(|alias| {
                interface::TypeAlias::new(
                    alias.name(),
                    alias.original_name(),
                    alias.type_().clone(),
                    alias.is_public() && !alias.is_external(),
                    alias.position().clone(),
                )
            })
            .collect(),
        module
            .definitions()
            .iter()
            .filter_map(|definition| {
                if definition.is_public() {
                    Some(interface::Declaration::new(
                        definition.name(),
                        definition.original_name(),
                        type_extractor::extract_from_lambda(definition.lambda()),
                        definition.position().clone(),
                    ))
                } else {
                    None
                }
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test;
    use hir::{
        test::{DefinitionFake, ModuleFake},
        types,
    };

    #[test]
    fn compile_empty_module() {
        assert_eq!(
            compile(&ir::Module::empty()),
            Ok(interface::Module::new(vec![], vec![], vec![]))
        );
    }

    #[test]
    fn compile_without_private_declaration() {
        assert_eq!(
            compile(
                &ir::Module::empty().set_definitions(vec![ir::Definition::fake(
                    "foo",
                    ir::Lambda::new(
                        vec![],
                        types::None::new(test::position()),
                        ir::None::new(test::position()),
                        test::position(),
                    ),
                    false,
                )])
            ),
            Ok(interface::Module::new(vec![], vec![], vec![]))
        );
    }
}