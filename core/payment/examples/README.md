## Payment API examples

### Startup

To start the API server (both provider & requestor) run the following commands:
```shell script
cd core/payment
cp ../../.env-template .env
cargo run --features=dummy-driver --example payment_api
```
To use GNT instead of dummy driver use `--features=gnt-driver` instead of `--features=dummy-driver`.

### Debit note flow

To test the whole flow start the API server (see above) and run the debit_note_flow
example in another terminal:
```shell script
cd core/payment
cargo run --features=dummy-driver --example debit_note_flow
```
(**NOTE:** The example expects a clean database so might need to remove `payment.db`
and restart the API server.)

##### Issue a debit node:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes`

Payload:
```json
{
  "activityId": "activity_id",
  "totalAmountDue": "1.123456789012345678",
  "usageCounterVector": {
    "comment": "This field can contain anything",
    "values": [1.222, 2.333, 4.555]
  },
  "paymentDueDate": "2020-02-05T15:07:45.956Z"
}
```
Don't forget to copy `debitNoteId` from the response!

##### Send the issued debit note to requestor:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes/<debitNoteId>/send`

##### See debit notes issued by the provider:  
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes`

##### See debit notes received by the requestor:  
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/debitNotes`

##### Accept a debit note:
`POST` `http://127.0.0.1:7465/payment-api/v1/requestor/debitNotes/<debitNoteId>/accept`

Payload:
```json
{
  "totalAmountAccepted": "1.123456789012345678",
  "allocationId": "<allocationId>"
}
```

##### Listen for requestor's debit note events:
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/debitNoteEvents?timeout=<seconds>`

##### Listen for provider's debit note events:
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/debitNoteEvents?timeout=<seconds>`

### Invoice flow

To test the whole flow start the API server (see above) and run the invoice_flow
example in another terminal:
```shell script
cargo run --features=dummy-driver --example invoice_flow
```
(**NOTE:** The example expects a clean database so might need to remove `payment.db`
and restart the API server.)

##### Issue an invoice:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/invoices`

Payload:
```json
{
  "agreementId": "agreement_id",
  "activityIds": ["activity_id1", "activity_id2"],
  "amount": "10.123456789012345678",
  "paymentDueDate": "2020-02-05T15:07:45.956Z"
}
```
Don't forget to copy `invoiceId` from the response!

##### Send the issued invoice to requestor:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/invoices/<invoiceId>/send`

##### See invoices issued by the provider:  
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/invoices`

##### See invoices received by the requestor:  
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/invoices`

##### Accept an invoice:
`POST` `http://127.0.0.1:7465/payment-api/v1/requestor/invoices/<invoiceId>/accept`

Payload:
```json
{
  "totalAmountAccepted": "10.123456789012345678",
  "allocationId": "<allocationId>"
}
```

##### Listen for requestor's invoice events:
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/invoiceEvents?timeout=<seconds>`

##### Listen for provider's invoice events:
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/invoiceEvents?timeout=<seconds>`

### Allocations

##### Create an allocation:  
`POST` `http://127.0.0.1:7465/payment-api/v1/requestor/allocations`

Payload:
```json
{
  "totalAmount": "100.123456789012345678",
  "timeout": "2020-02-17T11:42:56.739Z",
  "makeDeposit": false
}
```
Don't forget to copy `allocationId` from the response!

##### See all created allocations:
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/allocations`

##### Release an allocation:
`DELETE` `http://127.0.0.1:7465/payment-api/v1/requestor/allocations/<allocationId>`

### Payments

##### See requestor's (sent) payments:
`GET` `http://127.0.0.1:7465/payment-api/v1/requestor/payments`

##### See provider's (received) payments:
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/payments`

One can also listen for payments by adding `?timeout=<seconds>` parameter.
