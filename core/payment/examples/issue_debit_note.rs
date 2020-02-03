use serde_json;
use ya_model::payment::{InvoiceStatus, NewDebitNote};

fn main() {
    let input = r#"{
      "agreementId": "agreement_id",
      "activityId": "activity_id",
      "totalAmountDue": 1000,
      "usageCounterVector": {
        "comment": "This field can contain anything",
        "values": [1.222, 2.333, 4.555]
      },
      "creditAccountId": "0xdeadbeef",
      "paymentPlatform": "GNT",
      "paymentDueDate": "2020-02-05T15:07:45.956Z"
    }"#;
    let invoice = r#"{
      "agreementId": "agreement_id",
      "activityIds": ["activity_id1", "activity_id2"],
      "amount": 10000,
      "usageCounterVector": {
        "comment": "This field can contain anything",
        "values": [1.222, 2.333, 4.555]
      },
      "creditAccountId": "0xdeadbeef",
      "paymentPlatform": "GNT",
      "paymentDueDate": "2020-02-05T15:07:45.956Z"
    }"#;
    let new_debit_note: NewDebitNote = serde_json::from_str(input).unwrap();
    let status: InvoiceStatus = serde_json::from_str("\"ISSUED\"").unwrap();
}
