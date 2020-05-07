use ya_agreement_utils::{
    constraints,
    ClauseOperator::{And, Or},
    ConstraintKey, ConstraintValue, Constraints,
};

fn main() {
    /* without macro */
    let constraints_1 = Constraints::new_clause(
        And,
        vec![
            ConstraintKey::new("golem.inf.mem.gib").greater_than(ConstraintValue::new(0.5)),
            ConstraintKey::new("golem.inf.storage.gib").equal_to(ConstraintValue::new(1.0)),
            Constraints::new_clause(
                Or,
                vec![
                    ConstraintKey::new("golem.inf.mem.gib").greater_than(ConstraintValue::new(0.5)),
                    ConstraintKey::new("golem.inf.storage.gib")
                        .less_than(ConstraintValue::new(0.5)),
                    ConstraintKey::new("golem.single.key").into(),
                ],
            )
            .into(),
        ],
    )
    .and(Constraints::new_single(
        ConstraintKey::new("a").greater_than(ConstraintValue::new(100)),
    ));
    let constraints_2 = Constraints::new_clause(
        And,
        vec![
            ConstraintKey::new("golem.inf.mem.gib.a.b.c").greater_than(ConstraintValue::new(0.5)),
            ConstraintKey::new("golem.inf.storage.d.e").not_equal_to(ConstraintValue::new("abc")),
        ],
    );
    let constraints_and = constraints_1.clone().and(constraints_2.clone());
    let constraints_or = constraints_1.clone().or(constraints_2.clone());

    println!("And:\n{}", constraints_and.to_string());
    println!("Or:\n{}", constraints_or.to_string());

    println!("Iteration:");
    for expr in constraints_1 {
        println!("-> {}", expr)
    }

    let test_get = constraints!["a.b.c" > 3, "hello.world" == 10, "a.b.c" < 10];
    println!(
        "Get constraints filtered by key: {}",
        test_get.filter_by_key("a.b.c").unwrap()
    );

    /* with macro */
    let c = constraints![
        "golem.inf.mem.gib.macro" == 1,
        "only.key",
        "c.macro" < 3,
        constraints!["test.1" > 1, "test.2" != 2],
        "d.macro" == 20,
        "e.macro" < 10
    ]
    .without("c.macro")
    .or(constraints!["golem.macro.one" == 1, "golem.macro.two" < 2]);
    println!("Created with macro:\n{}", c);
}
