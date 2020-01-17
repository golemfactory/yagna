use diesel::prelude::*;

use ya_persistence::executor::DbExecutor;
use ya_persistence::schema::agreement::dsl as agreement_dsl;
use ya_persistence::schema::agreement_state::dsl as agreement_state_dsl;

fn main() -> anyhow::Result<()> {
    let data_dir = ya_service_api::default_data_dir()?;
    let db = DbExecutor::from_data_dir(&data_dir)?;
    let conn = db.conn()?;

    let _: anyhow::Result<()> = conn.transaction(|| {
        db.apply_migration(ya_persistence::migrations::run_with_output)
            .unwrap()?;

        diesel::insert_into(agreement_state_dsl::agreement_state)
            .values((
                agreement_state_dsl::id.eq(1),
                agreement_state_dsl::name.eq("dummy"),
            ))
            .execute(&conn)?;
        Ok(())
    });

    //    diesel::insert_into(agreement_dsl::agreement)
    //        .values((
    //            agreement_dsl::id.eq(1),
    //            agreement_dsl::natural_id.eq("0xAABB"),
    //            agreement_dsl::agreement_id.eq(Integer),
    //            agreement_dsl::state_id.eq(Integer),
    //            agreement_dsl::previous_note_id.eq(Nullable<Integer>),
    //            agreement_dsl::created_date.eq(Timestamp),
    //            agreement_dsl::activity_id.eq(Nullable<Int,eger>),
    //            agreement_dsl::total_amount_due.eq(Text),
    //            agreement_dsl::usage_counter_json.eq(Nullable<Text>),
    //            agreement_dsl::credit_account.eq(Text),
    //            agreement_dsl::payment_due_date.eq(Nullable<Timestamp>),
    //    ))
    //    .execute(&conn)?;

    Ok(())
}
