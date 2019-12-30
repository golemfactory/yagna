# Yagna persistence db schema

![alt text](ERD.png "Entity Relationship diagram")


## Market domain

Tables containing entities essential for the Market Protocol negotiations.

### `Agreement`

Table including all the attributes of an Agreement.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| StateId               | integer | Foreign key to AgreementState |
| DemandPropertiesJson  | TEXT    | JSON text specifying Demand Properties |
| DemandConstraintsJson | TEXT    | Demand Constraints criteria expression |
| OfferPropertiesJson   | TEXT    | JSON text specifying Offer Properties |
| OfferConstraintsJson  | TEXT    | Demand Constraints criteria expression |
| ProposedSignature     | TEXT    | Digital signature bitstream (Base64-encoded) of the Agreement artifact serialized after Confirm() operation on Requestor side |
| ApprovedSignature     | TEXT    | Digital signature bitstream (Base64-encoded) of the Agreement artifact serialized after Approve() operation on Provider side |
| CommittedSignature    | TEXT    | Digital signature bitstream (Base64-encoded) of the Agreement artifact serialized after Approved message is delivered to Requestor side and final committment is sent to Provider |

### `AgreementEvent`

Table that tracks all events related to the Agreement.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| AgreementId           | integer | Foreign key to Agreement |
| EventDate             | datetime| |
| EventTypeId           | integer | Foreign key to AgreementEventType |

### `AgreementEventType`

Reference data table indicating available Agreement Event types.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| Name                  | varchar(50) | Label |

### `AgreementState`

Reference data table indicating available Agreement States.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| Name                  | varchar(50) | Label |

## Activity domain

Tables containing entities essential for the Activity management and auditing.

### `Activity`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| NaturalId             | varchar(255) | |
| AgreementId           | integer | Foreign key to Agreement |
| StateId               | integer | Foreign key to ActivityState |


### `ActivityEvent`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| ActivityId            | integer | Foreign key to Activity |
| EventDate             | datetime| |
| EventTypeId           | integer | Foreign key to ActivityEventType |


### `ActivityEventType`

Reference data table indicating available Activity Event types.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| Name                  | varchar(50) | Label |

### `ActivityState`

Reference data table indicating available Activity states.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| Name                  | varchar(50) | Label |

## Payment domain

Tables containing entities essential for the Payment Protocol execution.

### `Allocation`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| NaturalId             | varchar(255) | |
| CreatedDate           | datetime | |
| Amount                | varchar(36) | |
| RemainingAmount       | varchar(36) | |
| IsDeposit             | char(1) | |


### `DebitNote`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| NaturalId             | varchar(255) | |
| AgreementId           | integer | Foreign key to Agreement |
| StateId               | integer | Foreign key to InvoiceDebitNoteState |
| PreviousNoteId        | integer | Foreign key to self |
| CreatedDate           | datetime | |
| ActivityId            | integer | Foreign key to Activity |
| TotalAmountDue        | varchar(36) | |
| UsageVectorJson       | TEXT | |
| CreditAccount         | varchar(255) | |
| PaymentDueDate        | datetime | |

### `Invoice`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| NaturalId             | varchar(255) | |
| StateId               | integer | Foreign key to InvoiceDebitNoteState |
| LastDebitNoteId       | integer | Foreign key to DebitNote |
| CreatedDate           | datetime | |
| AgreementId           | integer | Foreign key to Agreement |
| Amount                | varchar(36) | |
| UsageCounterJson      | varchar(255) | |
| CreditAccount         | varchar(255) | |
| PaymentDueDate        | datetime | |

### `InvoiceDebitNoteState`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| Name                  | varchar(50) | Label |

### `InvoiceXActivity`

Association table between Invoice and Activity.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| InvoiceId             | integer | Foreign key to Invoice |
| ActivityId            | integer | Foreign key to Activity |

### `Payment`

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| NaturalId             | varchar(255) | |
| Amount                | varchar(36) | |
| DebitAccount          | varchar(255) | |
| CreatedDate           | datetime | |

### `PaymentXDebitNote`

Association table between Payment and DebitNote.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| PaymentId             | integer | Foreign key to Payment |
| DebitNoteId           | integer | Foreign key to DebitNote |

### `PaymentXInvoice`

Association table between Payment and Invoice.

| Column                | Type    | Description |
|-----------------------|---------|-----------|
| Id                    | integer | Primary key, autoincrement |
| PaymentId             | integer | Foreign key to Payment |
| InvoiceId             | integer | Foreign key to Invoice |
