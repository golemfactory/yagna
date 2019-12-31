CREATE TABLE [Activity](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[AgreementId] [integer] NOT NULL,
	[StateId] [integer] NOT NULL,
    FOREIGN KEY([StateId]) REFERENCES [ActivityState] ([Id]),
    FOREIGN KEY([AgreementId]) REFERENCES [Agreement] ([Id])
);

CREATE TABLE [ActivityEvent](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[ActivityId] [integer] NOT NULL,
	[EventDate] [datetime] NOT NULL,
	[EventTypeId] [integer] NOT NULL,
    FOREIGN KEY([ActivityId]) REFERENCES [Activity] ([Id]),
    FOREIGN KEY([EventTypeId]) REFERENCES [ActivityEventType] ([Id])
);

CREATE TABLE [ActivityEventType](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[Name] [varchar](50) NOT NULL
);

CREATE TABLE [ActivityState](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[Name] [varchar](50) NOT NULL
);

CREATE TABLE [Agreement](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[StateId] [integer] NOT NULL,
	[DemandNaturalId] [varchar](255) NOT NULL,
	[DemandNodeId] [varchar](255) NOT NULL,
	[DemandPropertiesJson] TEXT NOT NULL,
	[DemandConstraintsJson] TEXT NOT NULL,
	[OfferNaturalId] [varchar](255) NOT NULL,
	[OfferNodeId] [varchar](255) NOT NULL,
	[OfferPropertiesJson] TEXT NOT NULL,
	[OfferConstraintsJson] TEXT NOT NULL,
	[ProposedSignature] TEXT NOT NULL,
	[ApprovedSignature] TEXT NOT NULL,
	[CommittedSignature] TEXT NOT NULL,
    FOREIGN KEY([StateId]) REFERENCES [AgreementState] ([Id])
);

CREATE TABLE [AgreementEvent](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[AgreementId] [integer] NOT NULL,
	[EventDate] [datetime] NOT NULL,
	[EventTypeId] [integer] NOT NULL,
    FOREIGN KEY([AgreementId]) REFERENCES [Agreement] ([Id]),
    FOREIGN KEY([EventTypeId]) REFERENCES [AgreementEventType] ([Id])
);

CREATE TABLE [AgreementEventType](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[Name] [varchar](50) NOT NULL
);

CREATE TABLE [AgreementState](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[Name] [varchar](50) NOT NULL
);

CREATE TABLE [Allocation](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[CreatedDate] [datetime] NOT NULL,
	[Amount] [varchar](36) NOT NULL,
	[RemainingAmount] [varchar](36) NOT NULL,
	[IsDeposit] [char](1) NOT NULL
);

CREATE TABLE [DebitNote](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[AgreementId] [integer] NOT NULL,
	[StateId] [integer] NOT NULL,
	[PreviousNoteId] [integer] NULL,
	[CreatedDate] [datetime] NOT NULL,
	[ActivityId] [integer] NULL,
	[TotalAmountDue] [varchar](36) NOT NULL,
	[UsageCounterJson] TEXT NULL,
	[CreditAccount] [varchar](255) NOT NULL,
	[PaymentDueDate] [datetime] NULL,
    FOREIGN KEY([ActivityId]) REFERENCES [Activity] ([Id]),
    FOREIGN KEY([AgreementId]) REFERENCES [Agreement] ([Id]),
    FOREIGN KEY([StateId]) REFERENCES [InvoiceDebitNoteState] ([Id])
);

CREATE TABLE [Invoice](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[StateId] [integer] NOT NULL,
	[LastDebitNoteId] [integer] NULL,
	[CreatedDate] [datetime] NOT NULL,
	[AgreementId] [integer] NOT NULL,
	[Amount] [varchar](36) NOT NULL,
	[UsageCounterJson] [varchar](255) NULL,
	[CreditAccount] [varchar](255) NOT NULL,
	[PaymentDueDate] [datetime] NOT NULL,
    FOREIGN KEY([AgreementId]) REFERENCES [Agreement] ([Id]),
    FOREIGN KEY([StateId]) REFERENCES [InvoiceDebitNoteState] ([Id])
);

CREATE TABLE [InvoiceDebitNoteState](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[Name] [varchar](50) NOT NULL
);

CREATE TABLE [InvoiceXActivity](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[InvoiceId] [integer] NOT NULL,
	[ActivityId] [integer] NOT NULL,
    FOREIGN KEY([ActivityId]) REFERENCES [Activity] ([Id]),
    FOREIGN KEY([InvoiceId]) REFERENCES [Invoice] ([Id])
);

CREATE TABLE [Payment](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[NaturalId] [varchar](255) NOT NULL,
	[Amount] [varchar](36) NOT NULL,
	[DebitAccount] [varchar](255) NOT NULL,
	[CreatedDate] [datetime] NOT NULL
);

CREATE TABLE [PaymentXDebitNote](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[PaymentId] [integer] NOT NULL,
	[DebitNoteId] [integer] NOT NULL,
    FOREIGN KEY([DebitNoteId]) REFERENCES [DebitNote] ([Id]),
    FOREIGN KEY([PaymentId]) REFERENCES [Payment] ([Id])
);

CREATE TABLE [PaymentXInvoice](
	[Id] [integer] NOT NULL PRIMARY KEY AUTOINCREMENT,
	[PaymentId] [integer] NOT NULL,
	[InvoiceId] [integer] NOT NULL,
    FOREIGN KEY([InvoiceId]) REFERENCES [Invoice] ([Id]),
    FOREIGN KEY([PaymentId]) REFERENCES [Payment] ([Id])
);

