use crate::{
    ast,
    hir::{analysis::variable_transformer, *},
    interface, types,
};
use std::collections::HashMap;

use super::name_qualifier;

pub fn compile(
    module: &Module,
    module_interfaces: &HashMap<ast::ModulePath, interface::Module>,
) -> Module {
    let names = module_interfaces
        .iter()
        .flat_map(|(path, module)| {
            module
                .type_definitions()
                .iter()
                .filter_map(|definition| {
                    if definition.elements().is_empty() && definition.is_public() {
                        Some((
                            name_qualifier::qualify(path, definition.original_name()),
                            definition.name().into(),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .chain(module.type_definitions().iter().filter_map(|definition| {
            if definition.elements().is_empty() && !definition.is_external() {
                Some((definition.original_name().into(), definition.name().into()))
            } else {
                None
            }
        }))
        .collect::<HashMap<String, String>>();

    variable_transformer::transform(module, &|variable| {
        if let Some(record_name) = names.get(variable.name()) {
            RecordConstruction::new(
                types::Record::new(record_name, variable.position().clone()),
                vec![],
                variable.position().clone(),
            )
            .into()
        } else {
            variable.clone().into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;
    use pretty_assertions::assert_eq;

    #[test]
    fn compile_singleton_record() {
        let type_definition =
            TypeDefinition::new("bar", "foo", vec![], false, false, false, Position::dummy());

        assert_eq!(
            compile(
                &Module::empty()
                    .set_type_definitions(vec![type_definition.clone()])
                    .set_definitions(vec![Definition::without_source(
                        "f",
                        Lambda::new(
                            vec![],
                            types::None::new(Position::dummy()),
                            Variable::new("foo", Position::dummy()),
                            Position::dummy()
                        ),
                        false
                    )]),
                &Default::default(),
            ),
            Module::empty()
                .set_type_definitions(vec![type_definition])
                .set_definitions(vec![Definition::without_source(
                    "f",
                    Lambda::new(
                        vec![],
                        types::None::new(Position::dummy()),
                        RecordConstruction::new(
                            types::Record::new("bar", Position::dummy()),
                            vec![],
                            Position::dummy()
                        ),
                        Position::dummy()
                    ),
                    false
                )])
        );
    }

    #[test]
    fn do_not_compile_non_singleton_record() {
        let type_definition = TypeDefinition::new(
            "bar",
            "foo",
            vec![types::RecordElement::new(
                "x",
                types::None::new(Position::dummy()),
            )],
            false,
            false,
            false,
            Position::dummy(),
        );
        let definition = Definition::without_source(
            "f",
            Lambda::new(
                vec![],
                types::None::new(Position::dummy()),
                Variable::new("foo", Position::dummy()),
                Position::dummy(),
            ),
            false,
        );

        assert_eq!(
            compile(
                &Module::empty()
                    .set_type_definitions(vec![type_definition.clone()])
                    .set_definitions(vec![definition.clone()]),
                &Default::default(),
            ),
            Module::empty()
                .set_type_definitions(vec![type_definition])
                .set_definitions(vec![definition])
        );
    }

    #[test]
    fn compile_imported_singleton_record() {
        assert_eq!(
            compile(
                &Module::empty().set_definitions(vec![Definition::without_source(
                    "f",
                    Lambda::new(
                        vec![],
                        types::None::new(Position::dummy()),
                        Variable::new("Foo'Foo", Position::dummy()),
                        Position::dummy()
                    ),
                    false
                )]),
                &vec![(
                    ast::InternalModulePath::new(vec!["Foo".into()]).into(),
                    interface::Module::new(
                        vec![interface::TypeDefinition::new(
                            "RealFoo",
                            "Foo",
                            vec![],
                            false,
                            true,
                            Position::dummy()
                        )],
                        vec![],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
            ),
            Module::empty().set_definitions(vec![Definition::without_source(
                "f",
                Lambda::new(
                    vec![],
                    types::None::new(Position::dummy()),
                    RecordConstruction::new(
                        types::Record::new("RealFoo", Position::dummy()),
                        vec![],
                        Position::dummy()
                    ),
                    Position::dummy()
                ),
                false
            )])
        );
    }

    #[test]
    fn do_not_compile_imported_non_singleton_record() {
        let definition = Definition::without_source(
            "f",
            Lambda::new(
                vec![],
                types::None::new(Position::dummy()),
                Variable::new("Foo'Foo", Position::dummy()),
                Position::dummy(),
            ),
            false,
        );

        assert_eq!(
            compile(
                &Module::empty().set_definitions(vec![definition.clone()]),
                &vec![(
                    ast::InternalModulePath::new(vec!["Foo".into()]).into(),
                    interface::Module::new(
                        vec![interface::TypeDefinition::new(
                            "RealFoo",
                            "Foo",
                            vec![types::RecordElement::new(
                                "x",
                                types::None::new(Position::dummy()),
                            )],
                            false,
                            true,
                            Position::dummy()
                        )],
                        vec![],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
            ),
            Module::empty().set_definitions(vec![definition])
        );
    }

    #[test]
    fn do_not_compile_imported_private_record() {
        let definition = Definition::without_source(
            "f",
            Lambda::new(
                vec![],
                types::None::new(Position::dummy()),
                Variable::new("Foo'Foo", Position::dummy()),
                Position::dummy(),
            ),
            false,
        );

        assert_eq!(
            compile(
                &Module::empty().set_definitions(vec![definition.clone()]),
                &vec![(
                    ast::InternalModulePath::new(vec!["Foo".into()]).into(),
                    interface::Module::new(
                        vec![interface::TypeDefinition::new(
                            "RealFoo",
                            "Foo",
                            vec![],
                            false,
                            false,
                            Position::dummy()
                        )],
                        vec![],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
            ),
            Module::empty().set_definitions(vec![definition])
        );
    }
}