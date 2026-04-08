#!/usr/bin/env node
// Minimal MCP stdio server for smoke testing.
// Exposes a single tool whose name is controlled by MCP_TOOL_NAME env var.
//
// Protocol: JSON-RPC 2.0 over stdin/stdout, one JSON object per line.

const readline = require("readline");

const TOOL_NAME = process.env.MCP_TOOL_NAME || "echo";
const SERVER_NAME = process.env.MCP_SERVER_NAME || "echo-server";

const rl = readline.createInterface({ input: process.stdin });

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

rl.on("line", (line) => {
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return;
  }

  const { id, method, params } = req;

  switch (method) {
    case "initialize":
      send({
        jsonrpc: "2.0",
        id,
        result: {
          protocolVersion: "2024-11-05",
          capabilities: { tools: {} },
          serverInfo: { name: SERVER_NAME, version: "1.0.0" },
        },
      });
      break;

    case "notifications/initialized":
      // no response needed
      break;

    case "tools/list":
      send({
        jsonrpc: "2.0",
        id,
        result: {
          tools: [
            {
              name: TOOL_NAME,
              description: `Echoes back the input message. Tool name: ${TOOL_NAME}`,
              inputSchema: {
                type: "object",
                properties: {
                  message: {
                    type: "string",
                    description: "The message to echo back",
                  },
                },
                required: ["message"],
              },
            },
          ],
        },
      });
      break;

    case "tools/call": {
      const toolName = params?.name;
      const message = params?.arguments?.message || "(empty)";
      send({
        jsonrpc: "2.0",
        id,
        result: {
          content: [
            {
              type: "text",
              text: `[${toolName}] echoed: ${message}`,
            },
          ],
        },
      });
      break;
    }

    default:
      send({
        jsonrpc: "2.0",
        id,
        error: { code: -32601, message: `Method not found: ${method}` },
      });
  }
});
