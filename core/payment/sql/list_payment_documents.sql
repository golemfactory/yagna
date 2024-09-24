SELECT pp.timestamp,
       pp.payee_addr,
       ppd.payment_id,
       ppd.owner_id,
       ppd.peer_id,
       ppd.agreement_id,
       ppd.invoice_id,
       ppd.activity_id,
       ppd.debit_note_id,
       ppd.amount
  FROM pay_payment_document ppd
  JOIN pay_payment pp ON ppd.owner_id = pp.owner_id AND ppd.peer_id = pp.peer_id AND ppd.payment_id = pp.id
  ORDER BY pp.timestamp desc
