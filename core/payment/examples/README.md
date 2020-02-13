### Payment API example

#### Startup

To start the provider:
```shell script
RUST_BACKTRACE=1 CENTRAL_NET_HOST=34.244.4.185:7464 cargo run --example payment_api -- provider
```

To start the requestor:
```shell script
RUST_BACKTRACE=1 CENTRAL_NET_HOST=34.244.4.185:7464 GSB_URL="tcp://127.0.0.1:8464" YAGNA_BUS_PORT=8464 YAGNA_HTTP_PORT=8465 cargo run --example payment_api -- requestor
```

#### Debit note flow

To issue a debit node:  
`POST` `http://127.0.0.1:7465/payment-api/v1/provider/debitNotes`

Payload:
```json
{
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
  "amount": 10000,
  "usageCounterVector": {
    "comment": "This field can contain anything",
    "values": [1.222, 2.333, 4.555]
  },
  "creditAccountId": "0xdeadbeef",
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
