import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { LspManager } from "../../lsp/LspManager.js";
import { URI } from "vscode-uri";
import fs from "fs";
import path from "path";

const rootDir = process.env.WORKSPACE_DIR || process.cwd();
const lspManager = new LspManager(rootDir);

const server = new Server(
  {
    name: "sera-symbol-server",
    version: "1.0.0",
  },
  {
    capabilities: {
      tools: {},
    },
  }
);

server.setRequestHandler(ListToolsRequestSchema, async () => {
  return {
    tools: [
      {
        name: "find_symbol",
        description: "Find a symbol (class, function, variable) by name in the codebase.",
        inputSchema: {
          type: "object",
          properties: {
            name: {
              type: "string",
              description: "The name of the symbol to find",
            },
          },
          required: ["name"],
        },
      },
      {
        name: "find_referencing_symbols",
        description: "Find all symbols that reference a given symbol at a specific location.",
        inputSchema: {
          type: "object",
          properties: {
            filePath: {
              type: "string",
              description: "The relative path to the file containing the symbol",
            },
            line: {
              type: "number",
              description: "The line number (0-indexed) where the symbol is located",
            },
            character: {
              type: "number",
              description: "The character position (0-indexed) where the symbol is located",
            },
          },
          required: ["filePath", "line", "character"],
        },
      },
      {
        name: "insert_after_symbol",
        description: "Insert content after a specific symbol in a file.",
        inputSchema: {
          type: "object",
          properties: {
            name: {
              type: "string",
              description: "The name of the symbol to insert after",
            },
            content: {
              type: "string",
              description: "The content to insert",
            },
            filePath: {
              type: "string",
              description: "Optional: The file path if there are multiple symbols with the same name",
            },
          },
          required: ["name", "content"],
        },
      },
    ],
  };
});

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;

  try {
    switch (name) {
      case "find_symbol": {
        const query = args?.name as string;
        // Use a generic file name to get the right LSP client
        const dummyFile = path.resolve(rootDir, "index.ts");
        const client = await lspManager.getClientForFile(dummyFile);
        if (!client) throw new Error("No LSP client found for typescript");

        const symbols = await client.getWorkspaceSymbols(query);
        return {
          content: [{ type: "text", text: JSON.stringify(symbols, null, 2) }],
        };
      }

      case "find_referencing_symbols": {
        const filePath = args?.filePath as string;
        const line = args?.line as number;
        const character = args?.character as number;

        const absolutePath = path.resolve(rootDir, filePath);
        const client = await lspManager.getClientForFile(absolutePath);
        if (!client) throw new Error(`No LSP client for ${filePath}`);

        const uri = URI.file(absolutePath).toString();
        await client.openFile(uri, getLanguageId(absolutePath));
        const references = await client.getReferences(uri, line, character);

        return {
          content: [{ type: "text", text: JSON.stringify(references, null, 2) }],
        };
      }

      case "insert_after_symbol": {
        const symbolName = args?.name as string;
        const insertContent = args?.content as string;
        const targetFilePath = args?.filePath as string | undefined;

        const dummyFile = path.resolve(rootDir, "index.ts");
        const client = await lspManager.getClientForFile(dummyFile);
        if (!client) throw new Error("No LSP client found for typescript");

        let symbols = await client.getWorkspaceSymbols(symbolName);
        if (!symbols || symbols.length === 0) {
          throw new Error(`Symbol "${symbolName}" not found`);
        }

        if (targetFilePath) {
          const absTarget = path.resolve(rootDir, targetFilePath);
          symbols = symbols.filter(s => {
            const uri = (s.location as any).uri || s.location;
            return URI.parse(uri).fsPath === absTarget;
          });
        }

        if (symbols.length === 0) {
          throw new Error(`Symbol "${symbolName}" not found in ${targetFilePath}`);
        }

        // Pick the best match (exact name match first)
        const exactMatch = symbols.find(s => s.name === symbolName) || symbols[0];
        if (!exactMatch) {
           throw new Error(`Symbol "${symbolName}" not found after filtering`);
        }
        const location = exactMatch.location as any;
        const uri = location.uri || location;
        const range = location.range;

        if (!range) {
          throw new Error(`Symbol "${symbolName}" has no range information`);
        }

        const absPath = URI.parse(uri).fsPath;

        // Path validation
        const resolvedPath = path.resolve(absPath);
        const resolvedRoot = path.resolve(rootDir);
        if (!resolvedPath.startsWith(resolvedRoot)) {
          throw new Error(`Access denied: Path ${absPath} is outside workspace`);
        }

        const content = fs.readFileSync(absPath, "utf-8");
        const lines = content.split("\n");

        // range.end.line is 0-indexed
        const endLine = range.end.line;
        const endChar = range.end.character;

        const lineContent = lines[endLine];
        if (lineContent === undefined) {
           throw new Error(`Line ${endLine} not found in file`);
        }
        lines[endLine] = lineContent.slice(0, endChar) + insertContent + lineContent.slice(endChar);

        fs.writeFileSync(absPath, lines.join("\n"), "utf-8");

        return {
          content: [{ type: "text", text: `Successfully inserted content after symbol "${symbolName}" in ${absPath}` }],
        };
      }

      default:
        throw new Error(`Unknown tool: ${name}`);
    }
  } catch (error: any) {
    return {
      isError: true,
      content: [{ type: "text", text: error.message }],
    };
  }
});

function getLanguageId(filePath: string) {
  const ext = path.extname(filePath);
  switch (ext) {
    case ".ts":
    case ".tsx":
      return "typescript";
    case ".js":
    case ".jsx":
      return "javascript";
    default:
      return "plaintext";
  }
}

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
