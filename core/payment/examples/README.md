### Payment API example

#### Startup

To start the provider:
```shell script
RUST_BACKTRACE=1 cargo run --example payment_api -- provider
```

To start the requestor:
```shell script
RUST_BACKTRACE=1 GSB_URL="tcp://127.0.0.1:8464" YAGNA_BUS_PORT=8464 YAGNA_HTTP_PORT=8465 cargo run --example payment_api -- requestor
```

#### Debit note flow

To issue a debit node:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes`

Payload:
```json
{
  "agreementId": "agreement_id",
  "activityId": "activity_id",
  "totalAmountDue": "1.123456789012345678",
  "usageCounterVector": {
    "comment": "This field can contain anything",
    "values": [1.222, 2.333, 4.555]
  },
  "creditAccountId": "0xd39a168f0480b8502c2531b2ffd8588c592d713a",
  "paymentPlatform": "GNT",
  "paymentDueDate": "2020-02-05T15:07:45.956Z"
}
```
Don't forget to copy `debitNoteId` from the response!

To send the issued debit note to requestor:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes/<debitNoteId>/send`

To see debit notes issued by the provider:  
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes`

To see debit notes received by the requestor:  
`GET` `http://127.0.0.1:8465/payment-api/v1/requestor/debitNotes`

#### Invoice flow

To issue an invoice:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/invoices`

Payload:
```json
{
  "agreementId": "agreement_id",
  "activityIds": ["activity_id1", "activity_id2"],
  "amount": "10.123456789012345678",
  "usageCounterVector": {
    "comment": "This field can contain anything",
    "values": [1.222, 2.333, 4.555]
  },
  "creditAccountId": "0xd39a168f0480b8502c2531b2ffd8588c592d713a",
  "paymentPlatform": "GNT",
  "paymentDueDate": "2020-02-05T15:07:45.956Z"
}
```
Don't forget to copy `invoiceId` from the response!

To send the issued invoice to requestor:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/invoices/<invoiceId>/send`

To see invoices issued by the provider:  
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/invoices`

To see invoices received by the requestor:  
`GET` `http://127.0.0.1:8465/payment-api/v1/requestor/invoices`

To accept an invoice:
`POST` `http://127.0.0.1:8465/payment-api/v1/requestor/invoices/<invoiceId>/accept`

Payload:
```json
{
  "totalAmountAccepted": "10.123456789012345678",
  "allocationId": "<allocationId>"
}
```

To listen for requestor's invoice events:
`GET` `http://127.0.0.1:8465/payment-api/v1/requestor/invoiceEvents?timeout=<seconds>`

To listen for provider's invoice events:
`GET` `http://127.0.0.1:8465/payment-api/v1/provider/invoiceEvents?timeout=<seconds>`

#### Allocations

To create an allocation:  
`POST` `http://127.0.0.1:8465/payment-api/v1/requestor/allocations`

Payload:
```json
{
  "totalAmount": "100.123456789012345678",
  "timeout": "2020-02-17T11:42:56.739Z",
  "makeDeposit": false
}
```
Don't forget to copy `allocationId` from the response!

To see all created allocations:
`GET` `http://127.0.0.1:8465/payment-api/v1/requestor/allocations`

To release an allocation:
`DELETE` `http://127.0.0.1:8465/payment-api/v1/requestor/allocations/<allocationId>`

#### Payments

To see requestor's (sent) payments:
`GET` `http://127.0.0.1:8465/payment-api/v1/requestor/payments`

To see provider's (received) payments:
`GET` `http://127.0.0.1:7465/payment-api/v1/provider/payments`

One can also listen for payments by adding `?timeout=<seconds>` parameter.
