use super::{environment_creator, type_context::TypeContext, type_extractor, CompileError};
use hir::{
    analysis::types::{type_canonicalizer, type_difference_calculator, union_type_creator},
    ir::*,
    types::{self, Type},
};
use std::collections::HashMap;

pub fn infer_types(module: &Module, type_context: &TypeContext) -> Result<Module, CompileError> {
    let variables = environment_creator::create_from_module(module);

    Ok(Module::new(
        module.type_definitions().to_vec(),
        module.type_aliases().to_vec(),
        module.foreign_declarations().to_vec(),
        module.declarations().to_vec(),
        module
            .definitions()
            .iter()
            .map(|definition| infer_definition(definition, &variables, type_context))
            .collect::<Result<_, _>>()?,
        module.position().clone(),
    ))
}

fn infer_definition(
    definition: &Definition,
    variables: &HashMap<String, Type>,
    type_context: &TypeContext,
) -> Result<Definition, CompileError> {
    Ok(Definition::new(
        definition.name(),
        definition.original_name(),
        infer_lambda(definition.lambda(), variables, type_context)?,
        definition.is_foreign(),
        definition.is_public(),
        definition.position().clone(),
    ))
}

fn infer_lambda(
    lambda: &Lambda,
    variables: &HashMap<String, Type>,
    type_context: &TypeContext,
) -> Result<Lambda, CompileError> {
    Ok(Lambda::new(
        lambda.arguments().to_vec(),
        lambda.result_type().clone(),
        infer_expression(
            lambda.body(),
            &variables
                .clone()
                .into_iter()
                .chain(
                    lambda
                        .arguments()
                        .iter()
                        .map(|argument| (argument.name().into(), argument.type_().clone())),
                )
                .collect(),
            type_context,
        )?,
        lambda.position().clone(),
    ))
}

fn infer_expression(
    expression: &Expression,
    variables: &HashMap<String, Type>,
    type_context: &TypeContext,
) -> Result<Expression, CompileError> {
    let infer_expression =
        |expression, variables: &_| infer_expression(expression, variables, type_context);

    Ok(match expression {
        Expression::Call(call) => {
            let function = infer_expression(call.function(), variables)?;

            Call::new(
                Some(type_extractor::extract_from_expression(
                    &function,
                    variables,
                    type_context,
                )?),
                function.clone(),
                call.arguments()
                    .iter()
                    .map(|argument| infer_expression(argument, variables))
                    .collect::<Result<_, _>>()?,
                call.position().clone(),
            )
            .into()
        }
        Expression::If(if_) => {
            let then = infer_expression(if_.then(), variables)?;
            let else_ = infer_expression(if_.else_(), variables)?;

            If::new(
                infer_expression(if_.condition(), variables)?,
                then,
                else_,
                if_.position().clone(),
            )
            .into()
        }
        Expression::IfList(if_) => {
            let list = infer_expression(if_.argument(), variables)?;
            let list_type = type_canonicalizer::canonicalize_list(
                &type_extractor::extract_from_expression(&list, variables, type_context)?,
                type_context.types(),
            )?
            .ok_or_else(|| CompileError::ListExpected(if_.argument().position().clone()))?;

            let then = infer_expression(
                if_.then(),
                &variables
                    .clone()
                    .into_iter()
                    .chain(vec![
                        (
                            if_.first_name().into(),
                            types::Function::new(
                                vec![],
                                list_type.element().clone(),
                                if_.position().clone(),
                            )
                            .into(),
                        ),
                        (if_.rest_name().into(), list_type.clone().into()),
                    ])
                    .collect(),
            )?;
            let else_ = infer_expression(if_.else_(), variables)?;

            IfList::new(
                Some(list_type.element().clone()),
                list,
                if_.first_name(),
                if_.rest_name(),
                then,
                else_,
                if_.position().clone(),
            )
            .into()
        }
        Expression::IfType(if_) => {
            let argument = infer_expression(if_.argument(), variables)?;
            let branches = if_
                .branches()
                .iter()
                .map(|branch| -> Result<_, CompileError> {
                    Ok(IfTypeBranch::new(
                        branch.type_().clone(),
                        infer_expression(
                            branch.expression(),
                            &variables
                                .clone()
                                .into_iter()
                                .chain(vec![(if_.name().into(), branch.type_().clone())])
                                .collect(),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;

            let else_ = if_
                .else_()
                .map(|branch| -> Result<_, CompileError> {
                    let type_ = type_difference_calculator::calculate(
                        &type_extractor::extract_from_expression(
                            &argument,
                            variables,
                            type_context,
                        )?,
                        &union_type_creator::create(
                            &if_.branches()
                                .iter()
                                .map(|branch| branch.type_().clone())
                                .collect::<Vec<_>>(),
                            if_.position(),
                        )
                        .unwrap(),
                        type_context.types(),
                    )?
                    .ok_or_else(|| CompileError::UnreachableCode(branch.position().clone()))?;

                    Ok(ElseBranch::new(
                        Some(type_.clone()),
                        infer_expression(
                            branch.expression(),
                            &variables
                                .clone()
                                .into_iter()
                                .chain(vec![(if_.name().into(), type_)])
                                .collect(),
                        )?,
                        branch.position().clone(),
                    ))
                })
                .transpose()?;

            IfType::new(
                if_.name(),
                argument,
                branches,
                else_,
                if_.position().clone(),
            )
            .into()
        }
        Expression::Lambda(lambda) => infer_lambda(lambda, variables, type_context)?.into(),
        Expression::Let(let_) => {
            let bound_expression = infer_expression(let_.bound_expression(), variables)?;
            let bound_type = type_extractor::extract_from_expression(
                &bound_expression,
                variables,
                type_context,
            )?;

            Let::new(
                let_.name().map(String::from),
                Some(bound_type.clone()),
                bound_expression,
                infer_expression(
                    let_.expression(),
                    &variables
                        .clone()
                        .into_iter()
                        .chain(let_.name().map(|name| (name.into(), bound_type)))
                        .collect(),
                )?,
                let_.position().clone(),
            )
            .into()
        }
        Expression::List(list) => List::new(
            list.type_().clone(),
            list.elements()
                .iter()
                .map(|element| {
                    Ok(match element {
                        ListElement::Multiple(element) => {
                            ListElement::Multiple(infer_expression(element, variables)?)
                        }
                        ListElement::Single(element) => {
                            ListElement::Single(infer_expression(element, variables)?)
                        }
                    })
                })
                .collect::<Result<_, CompileError>>()?,
            list.position().clone(),
        )
        .into(),
        Expression::Operation(operation) => match operation {
            Operation::Arithmetic(operation) => ArithmeticOperation::new(
                operation.operator(),
                infer_expression(operation.lhs(), variables)?,
                infer_expression(operation.rhs(), variables)?,
                operation.position().clone(),
            )
            .into(),
            Operation::Boolean(operation) => BooleanOperation::new(
                operation.operator(),
                infer_expression(operation.lhs(), variables)?,
                infer_expression(operation.rhs(), variables)?,
                operation.position().clone(),
            )
            .into(),
            Operation::Equality(operation) => {
                let lhs = infer_expression(operation.lhs(), variables)?;
                let rhs = infer_expression(operation.rhs(), variables)?;

                EqualityOperation::new(
                    Some(
                        types::Union::new(
                            type_extractor::extract_from_expression(&lhs, variables, type_context)?,
                            type_extractor::extract_from_expression(&rhs, variables, type_context)?,
                            operation.position().clone(),
                        )
                        .into(),
                    ),
                    operation.operator(),
                    lhs,
                    rhs,
                    operation.position().clone(),
                )
                .into()
            }
            Operation::Not(operation) => NotOperation::new(
                infer_expression(operation.expression(), variables)?,
                operation.position().clone(),
            )
            .into(),
            Operation::Order(operation) => OrderOperation::new(
                operation.operator(),
                infer_expression(operation.lhs(), variables)?,
                infer_expression(operation.rhs(), variables)?,
                operation.position().clone(),
            )
            .into(),
            Operation::Try(operation) => {
                let position = operation.position();
                let expression = infer_expression(operation.expression(), variables)?;
                let error_type = types::Reference::new(
                    &type_context.error_type_configuration().error_type_name,
                    position.clone(),
                )
                .into();

                TryOperation::new(
                    Some(
                        if let Some(type_) = type_difference_calculator::calculate(
                            &type_extractor::extract_from_expression(
                                &expression,
                                variables,
                                type_context,
                            )?,
                            &error_type,
                            type_context.types(),
                        )? {
                            if type_.is_any() {
                                return Err(CompileError::UnionTypeExpected(
                                    expression.position().clone(),
                                ));
                            } else {
                                type_
                            }
                        } else {
                            return Err(CompileError::UnionTypeExpected(
                                expression.position().clone(),
                            ));
                        },
                    ),
                    expression,
                    position.clone(),
                )
                .into()
            }
        },
        Expression::RecordConstruction(construction) => RecordConstruction::new(
            construction.type_().clone(),
            construction
                .elements()
                .iter()
                .map(|element| {
                    Ok(RecordElement::new(
                        element.name(),
                        infer_expression(element.expression(), variables)?,
                        element.position().clone(),
                    ))
                })
                .collect::<Result<_, CompileError>>()?,
            construction.position().clone(),
        )
        .into(),
        Expression::RecordDeconstruction(deconstruction) => {
            let record = infer_expression(deconstruction.record(), variables)?;

            RecordDeconstruction::new(
                Some(type_extractor::extract_from_expression(
                    &record,
                    variables,
                    type_context,
                )?),
                record,
                deconstruction.element_name(),
                deconstruction.position().clone(),
            )
            .into()
        }
        Expression::RecordUpdate(update) => RecordUpdate::new(
            update.type_().clone(),
            infer_expression(update.record(), variables)?,
            update
                .elements()
                .iter()
                .map(|element| {
                    Ok(RecordElement::new(
                        element.name(),
                        infer_expression(element.expression(), variables)?,
                        element.position().clone(),
                    ))
                })
                .collect::<Result<_, CompileError>>()?,
            update.position().clone(),
        )
        .into(),
        Expression::Thunk(thunk) => Thunk::new(
            Some(type_extractor::extract_from_expression(
                thunk.expression(),
                variables,
                type_context,
            )?),
            infer_expression(thunk.expression(), variables)?,
            thunk.position().clone(),
        )
        .into(),
        Expression::TypeCoercion(coercion) => TypeCoercion::new(
            coercion.from().clone(),
            coercion.to().clone(),
            infer_expression(coercion.argument(), variables)?,
            coercion.position().clone(),
        )
        .into(),
        Expression::Boolean(_)
        | Expression::None(_)
        | Expression::Number(_)
        | Expression::String(_)
        | Expression::Variable(_) => expression.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error_type_configuration::ERROR_TYPE_CONFIGURATION,
        list_type_configuration::LIST_TYPE_CONFIGURATION,
        string_type_configuration::STRING_TYPE_CONFIGURATION, test,
    };
    use hir::test::{DefinitionFake, ModuleFake, TypeDefinitionFake};
    use pretty_assertions::assert_eq;

    fn infer_module(module: &Module) -> Result<Module, CompileError> {
        infer_types(
            module,
            &TypeContext::new(
                module,
                &LIST_TYPE_CONFIGURATION,
                &STRING_TYPE_CONFIGURATION,
                &ERROR_TYPE_CONFIGURATION,
            ),
        )
    }

    #[test]
    fn infer_empty_module() {
        infer_module(&Module::empty()).unwrap();
    }

    #[test]
    fn infer_call() {
        assert_eq!(
            infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    Call::new(
                        None,
                        Variable::new("x", test::position()),
                        vec![],
                        test::position()
                    ),
                    test::position(),
                ),
                false,
            )],)),
            Ok(Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    Call::new(
                        Some(
                            types::Function::new(
                                vec![],
                                types::None::new(test::position()),
                                test::position()
                            )
                            .into()
                        ),
                        Variable::new("x", test::position()),
                        vec![],
                        test::position()
                    ),
                    test::position(),
                ),
                false,
            )],))
        );
    }

    #[test]
    fn infer_equality_operation() {
        assert_eq!(
            infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    EqualityOperation::new(
                        None,
                        EqualityOperator::Equal,
                        None::new(test::position()),
                        None::new(test::position()),
                        test::position()
                    ),
                    test::position(),
                ),
                false,
            )],)),
            Ok(Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    EqualityOperation::new(
                        Some(
                            types::Union::new(
                                types::None::new(test::position()),
                                types::None::new(test::position()),
                                test::position()
                            )
                            .into()
                        ),
                        EqualityOperator::Equal,
                        None::new(test::position()),
                        None::new(test::position()),
                        test::position()
                    ),
                    test::position(),
                ),
                false,
            )],))
        );
    }

    #[test]
    fn infer_let() {
        assert_eq!(
            infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    Let::new(
                        Some("x".into()),
                        None,
                        None::new(test::position()),
                        Variable::new("x", test::position()),
                        test::position(),
                    ),
                    test::position(),
                ),
                false,
            )],)),
            Ok(Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    types::None::new(test::position()),
                    Let::new(
                        Some("x".into()),
                        Some(types::None::new(test::position()).into()),
                        None::new(test::position()),
                        Variable::new("x", test::position()),
                        test::position(),
                    ),
                    test::position(),
                ),
                false,
            )],))
        );
    }

    #[test]
    fn infer_let_with_call() {
        let declaration = Declaration::new(
            "f",
            types::Function::new(vec![], types::None::new(test::position()), test::position()),
            test::position(),
        );

        assert_eq!(
            infer_module(
                &Module::empty()
                    .set_declarations(vec![declaration.clone()])
                    .set_definitions(vec![Definition::fake(
                        "x",
                        Lambda::new(
                            vec![],
                            types::None::new(test::position()),
                            Let::new(
                                Some("x".into()),
                                None,
                                Call::new(
                                    None,
                                    Variable::new("f", test::position()),
                                    vec![],
                                    test::position()
                                ),
                                Variable::new("x", test::position()),
                                test::position(),
                            ),
                            test::position(),
                        ),
                        false,
                    )],)
            ),
            Ok(Module::empty()
                .set_declarations(vec![declaration.clone()])
                .set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![],
                        types::None::new(test::position()),
                        Let::new(
                            Some("x".into()),
                            Some(types::None::new(test::position()).into()),
                            Call::new(
                                Some(declaration.type_().clone().into()),
                                Variable::new("f", test::position()),
                                vec![],
                                test::position()
                            ),
                            Variable::new("x", test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )]))
        );
    }

    #[test]
    fn infer_record_deconstruction() {
        let type_definition = TypeDefinition::new(
            "r",
            "",
            vec![types::RecordElement::new(
                "x",
                types::None::new(test::position()),
            )],
            false,
            false,
            false,
            test::position(),
        );

        assert_eq!(
            infer_module(
                &Module::empty()
                    .set_type_definitions(vec![type_definition.clone()])
                    .set_definitions(vec![Definition::fake(
                        "x",
                        Lambda::new(
                            vec![Argument::new(
                                "x",
                                types::Record::new("r", test::position())
                            )],
                            types::None::new(test::position()),
                            RecordDeconstruction::new(
                                None,
                                Variable::new("x", test::position()),
                                "x",
                                test::position()
                            ),
                            test::position(),
                        ),
                        false,
                    )])
            ),
            Ok(Module::empty()
                .set_type_definitions(vec![type_definition])
                .set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new(
                            "x",
                            types::Record::new("r", test::position())
                        )],
                        types::None::new(test::position()),
                        RecordDeconstruction::new(
                            Some(types::Record::new("r", test::position()).into()),
                            Variable::new("x", test::position()),
                            "x",
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )]))
        );
    }

    #[test]
    fn infer_thunk() {
        let none_type = types::None::new(test::position());

        assert_eq!(
            infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    none_type.clone(),
                    Thunk::new(None, None::new(test::position()), test::position()),
                    test::position(),
                ),
                false,
            )])),
            Ok(Module::empty().set_definitions(vec![Definition::fake(
                "x",
                Lambda::new(
                    vec![],
                    none_type.clone(),
                    Thunk::new(
                        Some(none_type.into()),
                        None::new(test::position()),
                        test::position()
                    ),
                    test::position(),
                ),
                false,
            )]))
        );
    }

    mod if_type {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn infer_else_branch_type_of_none() {
            let union_type = types::Union::new(
                types::Number::new(test::position()),
                types::None::new(test::position()),
                test::position(),
            );
            let branches = vec![IfTypeBranch::new(
                types::Number::new(test::position()),
                None::new(test::position()),
            )];

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type.clone())],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches.clone(),
                            Some(ElseBranch::new(
                                None,
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],)),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type)],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches,
                            Some(ElseBranch::new(
                                Some(types::None::new(test::position()).into()),
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn infer_else_branch_type_of_union() {
            let union_type = types::Union::new(
                types::Union::new(
                    types::Number::new(test::position()),
                    types::Boolean::new(test::position()),
                    test::position(),
                ),
                types::None::new(test::position()),
                test::position(),
            );
            let branches = vec![IfTypeBranch::new(
                types::Number::new(test::position()),
                None::new(test::position()),
            )];

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type.clone())],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches.clone(),
                            Some(ElseBranch::new(
                                None,
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],)),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type)],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches,
                            Some(ElseBranch::new(
                                Some(
                                    types::Union::new(
                                        types::Boolean::new(test::position()),
                                        types::None::new(test::position()),
                                        test::position(),
                                    )
                                    .into()
                                ),
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn infer_else_branch_type_with_bound_variable() {
            let function_type =
                types::Function::new(vec![], types::None::new(test::position()), test::position());
            let union_type = types::Union::new(
                function_type.clone(),
                types::None::new(test::position()),
                test::position(),
            );
            let branches = vec![IfTypeBranch::new(
                types::None::new(test::position()),
                None::new(test::position()),
            )];

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type.clone())],
                        types::None::new(test::position()),
                        IfType::new(
                            "y",
                            Variable::new("x", test::position()),
                            branches.clone(),
                            Some(ElseBranch::new(
                                None,
                                Call::new(
                                    None,
                                    Variable::new("y", test::position()),
                                    vec![],
                                    test::position()
                                ),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],)),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type)],
                        types::None::new(test::position()),
                        IfType::new(
                            "y",
                            Variable::new("x", test::position()),
                            branches,
                            Some(ElseBranch::new(
                                Some(function_type.clone().into()),
                                Call::new(
                                    Some(function_type.into()),
                                    Variable::new("y", test::position()),
                                    vec![],
                                    test::position()
                                ),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn infer_else_branch_type_of_any() {
            let any_type = types::Any::new(test::position());
            let branches = vec![IfTypeBranch::new(
                types::Number::new(test::position()),
                None::new(test::position()),
            )];

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", any_type.clone())],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches.clone(),
                            Some(ElseBranch::new(
                                None,
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],)),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", any_type.clone())],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            branches,
                            Some(ElseBranch::new(
                                Some(any_type.into()),
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn fail_to_infer_else_branch_type_due_to_unreachable_code() {
            let union_type = types::Union::new(
                types::Number::new(test::position()),
                types::None::new(test::position()),
                test::position(),
            );

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "x",
                    Lambda::new(
                        vec![Argument::new("x", union_type)],
                        types::None::new(test::position()),
                        IfType::new(
                            "x",
                            Variable::new("x", test::position()),
                            vec![
                                IfTypeBranch::new(
                                    types::Number::new(test::position()),
                                    None::new(test::position()),
                                ),
                                IfTypeBranch::new(
                                    types::None::new(test::position()),
                                    None::new(test::position()),
                                )
                            ],
                            Some(ElseBranch::new(
                                None,
                                None::new(test::position()),
                                test::position()
                            )),
                            test::position()
                        ),
                        test::position(),
                    ),
                    false,
                )],)),
                Err(CompileError::UnreachableCode(test::position()))
            );
        }
    }

    mod try_operation {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn infer() {
            let union_type = types::Union::new(
                types::None::new(test::position()),
                types::Reference::new("error", test::position()),
                test::position(),
            );
            let module = Module::empty().set_type_definitions(vec![TypeDefinition::fake(
                "error",
                vec![],
                false,
                false,
                false,
            )]);

            assert_eq!(
                infer_module(&module.set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", union_type.clone())],
                        union_type.clone(),
                        TryOperation::new(
                            None,
                            Variable::new("x", test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )])),
                Ok(module.set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", union_type.clone())],
                        union_type,
                        TryOperation::new(
                            Some(types::None::new(test::position()).into()),
                            Variable::new("x", test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn fail_to_infer_with_error() {
            let error_type = types::Reference::new("error", test::position());

            assert_eq!(
                infer_module(
                    &Module::empty()
                        .set_type_definitions(vec![TypeDefinition::fake(
                            "error",
                            vec![],
                            false,
                            false,
                            false,
                        )])
                        .set_definitions(vec![Definition::fake(
                            "f",
                            Lambda::new(
                                vec![Argument::new("x", error_type.clone())],
                                error_type,
                                TryOperation::new(
                                    None,
                                    Variable::new("x", test::position()),
                                    test::position(),
                                ),
                                test::position(),
                            ),
                            false,
                        )],)
                ),
                Err(CompileError::UnionTypeExpected(test::position()))
            );
        }
    }

    mod if_list {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn infer() {
            let list_type = types::List::new(types::None::new(test::position()), test::position());

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type.clone())],
                        types::None::new(test::position()),
                        IfList::new(
                            None,
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Variable::new("y", test::position()),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )])),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type)],
                        types::None::new(test::position()),
                        IfList::new(
                            Some(types::None::new(test::position()).into()),
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Variable::new("y", test::position()),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn infer_with_first_name_in_let() {
            let list_type = types::List::new(types::None::new(test::position()), test::position());

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type.clone())],
                        types::None::new(test::position()),
                        IfList::new(
                            None,
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Let::new(
                                Some("z".into()),
                                None,
                                Variable::new("y", test::position()),
                                Variable::new("z", test::position()),
                                test::position()
                            ),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )])),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type)],
                        types::None::new(test::position()),
                        IfList::new(
                            Some(types::None::new(test::position()).into()),
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Let::new(
                                Some("z".into()),
                                Some(
                                    types::Function::new(
                                        vec![],
                                        types::None::new(test::position()),
                                        test::position()
                                    )
                                    .into()
                                ),
                                Variable::new("y", test::position()),
                                Variable::new("z", test::position()),
                                test::position()
                            ),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }

        #[test]
        fn infer_with_rest_name_in_let() {
            let list_type = types::List::new(types::None::new(test::position()), test::position());

            assert_eq!(
                infer_module(&Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type.clone())],
                        types::None::new(test::position()),
                        IfList::new(
                            None,
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Let::new(
                                Some("z".into()),
                                None,
                                Variable::new("ys", test::position()),
                                Variable::new("z", test::position()),
                                test::position()
                            ),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )])),
                Ok(Module::empty().set_definitions(vec![Definition::fake(
                    "f",
                    Lambda::new(
                        vec![Argument::new("x", list_type)],
                        types::None::new(test::position()),
                        IfList::new(
                            Some(types::None::new(test::position()).into()),
                            Variable::new("x", test::position()),
                            "y",
                            "ys",
                            Let::new(
                                Some("z".into()),
                                Some(
                                    types::List::new(
                                        types::None::new(test::position()),
                                        test::position()
                                    )
                                    .into()
                                ),
                                Variable::new("ys", test::position()),
                                Variable::new("z", test::position()),
                                test::position()
                            ),
                            None::new(test::position()),
                            test::position(),
                        ),
                        test::position(),
                    ),
                    false,
                )],))
            );
        }
    }
}