import {
  Command,
  CommandGroup,
  number,
  string,
} from "https://deno.land/x/clay/mod.ts";
import { BigDenary } from "https://deno.land/x/bigdenary/mod.ts";

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
type Role = "provider" | "requestor";

async function doRequest(
  method: Method,
  role: Role,
  path: string,
  body?: Object,
) {
  const url = `http://127.0.0.1:7465/${role}/payment-api/v1/${path}`;
  const headers = { "Accept": "application/json" };
  const hasBody = typeof body !== "undefined";
  if (hasBody) {
    headers["Content-Type"] = "application/json";
  }

  console.log(`\n\n%c${method} ${url}\n`, "color: blue; font-weight: bold;");
  if (hasBody) {
    console.log(`%c${JSON.stringify(body, null, 2)}`, "color: blue");
  }
  let init = { method, headers };
  if (body != null) {
    init["body"] = JSON.stringify(body);
  }
  let b = await fetch(url, init);
  let { status, statusText, headers: out_headers } = b;
  const output_ct = out_headers.get("Content-Type");
  console.log(
    `\n\n%cHTTP/1.1 ${status} ${statusText}`,
    "color: green; font-weight: bold",
  );
  if (output_ct) {
    console.log(`%ccontent-type: ${output_ct}\n`, "color: green");
  } else {
  }
  if (output_ct === "application/json") {
    let data = await b.json();
    console.log(`%c${JSON.stringify(data, null, 2)}`, "color: green");
  } else {
    console.log(`%c${await b.text()}\n`, "color: green");
  }
}

const allocationActions = {
  async create(args) {
    await doRequest("POST", "requestor", `allocations`, {
      totalAmount: args.amount,
    });
  },

  async delete({ id }) {
    await doRequest("DELETE", "requestor", `allocations/${id}`);
  },

  async list() {
    await doRequest("GET", "requestor", `allocations`);
  },

  async show({ id }) {
    await doRequest("GET", "requestor", `allocations/${id}`);
  },

  async update({ id, ...flags }) {
    await doRequest("PUT", "requestor", `allocations/${id}`, { ...flags });
  },
};

const requestorDebitNoteActions = {
  async list() {
    await doRequest("GET", "requestor", "debitNotes");
  },

  async reject({ id, reason, ...args }) {
    await doRequest("POST", "requestor", `debitNotes/${id}/reject`, {
      rejectionReason: reason,
      totalAmountAccepted: args["totalAmountAccepted"] || 0,
    });
  },

  async accept({ id, ...args }) {
    await doRequest("POST", "requestor", `debitNotes/${id}/accept`, { ... args });
  }
};

const providerDebitNoteActions = {
  "create": async (args) => {
    await doRequest("POST", "provider", "debitNotes", {
      "activityId": args.activityId || "activity_id",
      "totalAmountDue": args.totalAmountDue || "1.123456789012345678",
      "usageCounterVector": {
        "comment": "This field can contain anything",
        "values": [1.222, 2.333, 4.555],
      },
      "paymentDueDate": args['non-payable'] ? null : (args.paymentDueDate || "2020-02-05T15:07:45.956Z"),
    });
  },
  "send": async (args) => {
    await doRequest("POST", "provider", `debitNotes/${args.id}/send`);
  },
  "list": async (args) => {
    await doRequest("GET", "provider", "debitNotes");
  },

}

async function main() {
  let createProviderGroup = () => {
    const dn_create = new Command("createDebitNote")
        .flag("non-payable")
      .optional(string, "activityId", { flags: ["activity-id"] })
      .optional(string, "totalAmountDue", { flags: ["totalAmountDue"] })
      .optional(string, "paymentDueDate", { flags: ["paymentDueDate"] });

    const dn_send = new Command("send-debit-note").required(string, "id");

    const dn_list = new Command("List Debit Notes");

    const debit_note = new CommandGroup("debit note management")
        .subcommand("create", dn_create)
        .subcommand("send", dn_send)
        .subcommand("list", dn_list);

    return new CommandGroup("provider")
      .subcommand("debit-note", debit_note)
  };

  let createRequestorGroup = () => {
    // allocation
    const allocation_create = new Command("Create Allocation")
      .required(number, "amount")
      .optional(string, "network", { flags: ["network-id", "n"] });
    const allocation_list = new Command("List Allocations");
    const allocation_get = new Command("Get Allocation").required(string, "id");
    const allocation_update = new Command("Update/Touch allocation")
      .required(string, "id")
      .optional(string, "totalAmount", { flags: ["totalAmount", "a"] });
    const allocation_delete = new Command("Delete Allocation").required(
      string,
      "id",
    );
    const allocation = new CommandGroup("Allocation Managment")
      .subcommand("create", allocation_create)
      .subcommand("list", allocation_list)
      .subcommand("show", allocation_get)
      .subcommand("update", allocation_update)
      .subcommand("delete", allocation_delete);

    // debit note
    const dn_reject = new Command("reject-debit-note")
      .required(string, "id")
      .required(string, "reason")
      .optional(string, "totalAmountAccepted", { flags: ["a"] });

    const dn_accept = new Command("accept debit note")
        .required(string, "id")
        .required(string, "allocationId", { flags: ["c"] })
        .optional(string, "totalAmountAccepted", { flags: ["a"] })


    const dn_list = new Command("List Debit Notes");
    const debit_note = new CommandGroup("debit note management")
        .subcommand("accept", dn_accept)
      .subcommand("reject", dn_reject)
      .subcommand("list", dn_list);

    return new CommandGroup("requestor")
      .subcommand("allocation", allocation)
      .subcommand("debit-note", debit_note);
  };

  const providerActions = {
    "debit-note": async (args) => {
      return await applyActions(providerDebitNoteActions, args);
    },

  };

  const requestorActions = {
    "debit-note": async (args) => {
      return await applyActions(requestorDebitNoteActions, args);
    },

    async allocation(args) {
      return await applyActions(allocationActions, args);
    },
  };

  async function applyActions(actions, args) {
    for (const key in args) {
      if (key in actions) {
        await actions[key](args[key]);
      } else {
        console.error(`unknown command: ${key}`);
      }
    }
  }

  const args = new CommandGroup("payment cli")
    .subcommand("provider", createProviderGroup())
    .subcommand("requestor", createRequestorGroup());

  const pargs = args.run();
  if (pargs.provider) {
    await applyActions(providerActions, pargs.provider);
  }
  if (pargs.requestor) {
    await applyActions(requestorActions, pargs.requestor);
  }
}

await main();
