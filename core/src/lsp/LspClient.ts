import { spawn, type ChildProcess } from 'child_process';
import path from 'path';
import { Logger } from '../lib/logger.js';

const logger = new Logger('LspClient');
import * as rpc from 'vscode-jsonrpc/node.js';
import {
  InitializeRequest,
  type InitializeParams,
  InitializedNotification,
  DefinitionRequest,
  type DefinitionParams,
  ReferencesRequest,
  type ReferenceParams,
  DocumentSymbolRequest,
  type DocumentSymbolParams,
  WorkspaceSymbolRequest,
  type WorkspaceSymbolParams,
  type Location,
  type SymbolInformation,
  type DocumentSymbol,
  type WorkspaceSymbol,
  DidOpenTextDocumentNotification,
  type DidOpenTextDocumentParams
} from 'vscode-languageserver-protocol';
import type { LspClientOptions } from './types.js';
import fs from 'fs';
import { URI } from 'vscode-uri';

export class LspClient {
  private process: ChildProcess | null = null;
  private connection: rpc.MessageConnection | null = null;
  private options: LspClientOptions;
  private openedFiles: Set<string> = new Set();

  constructor(options: LspClientOptions) {
    this.options = options;
  }

  async start(): Promise<void> {
    const env = { ...process.env };
    if (this.options.serverCommand === 'typescript-language-server') {
      // Ensure local node_modules/.bin is in PATH for the LSP server
      const localBin = path.resolve(process.cwd(), 'node_modules', '.bin');
      env.PATH = `${localBin}${path.delimiter}${env.PATH}`;
    }

    this.process = spawn(this.options.serverCommand, this.options.serverArgs, { env });

    this.connection = rpc.createMessageConnection(
      new rpc.StreamMessageReader(this.process.stdout!),
      new rpc.StreamMessageWriter(this.process.stdin!)
    );

    this.connection.listen();

    const initializeParams: InitializeParams = {
      processId: process.pid,
      rootUri: this.options.rootUri,
      rootPath: URI.parse(this.options.rootUri).fsPath,
      capabilities: {
        textDocument: {
          definition: { dynamicRegistration: true },
          references: { dynamicRegistration: true },
          documentSymbol: { dynamicRegistration: true }
        }
      },
      workspaceFolders: null
    };

    try {
      await this.connection.sendRequest(InitializeRequest.method, initializeParams);
      await this.connection.sendNotification(InitializedNotification.method, {});
    } catch (error) {
      logger.error('Failed to initialize LSP server:', error);
      throw error;
    }
  }

  async openFile(uri: string, languageId: string): Promise<void> {
    if (!this.connection) throw new Error('LSP connection not established');
    if (this.openedFiles.has(uri)) return;

    const filePath = URI.parse(uri).fsPath;
    const content = fs.readFileSync(filePath, 'utf-8');

    const params: DidOpenTextDocumentParams = {
      textDocument: {
        uri,
        languageId,
        version: 1,
        text: content
      }
    };

    await this.connection.sendNotification(DidOpenTextDocumentNotification.method, params);
    this.openedFiles.add(uri);
  }

  async getDefinition(uri: string, line: number, character: number): Promise<Location | Location[] | null> {
    if (!this.connection) throw new Error('LSP connection not established');

    const params: DefinitionParams = {
      textDocument: { uri },
      position: { line, character }
    };

    return this.connection.sendRequest(DefinitionRequest.method, params) as Promise<Location | Location[] | null>;
  }

  async getReferences(uri: string, line: number, character: number): Promise<Location[] | null> {
    if (!this.connection) throw new Error('LSP connection not established');

    const params: ReferenceParams = {
      textDocument: { uri },
      position: { line, character },
      context: { includeDeclaration: true }
    };

    return this.connection.sendRequest(ReferencesRequest.method, params) as Promise<Location[] | null>;
  }

  async getDocumentSymbols(uri: string): Promise<SymbolInformation[] | DocumentSymbol[] | null> {
    if (!this.connection) throw new Error('LSP connection not established');

    const params: DocumentSymbolParams = {
      textDocument: { uri }
    };

    return this.connection.sendRequest(DocumentSymbolRequest.method, params) as Promise<SymbolInformation[] | DocumentSymbol[] | null>;
  }

  async getWorkspaceSymbols(query: string): Promise<WorkspaceSymbol[] | null> {
    if (!this.connection) throw new Error('LSP connection not established');

    const params: WorkspaceSymbolParams = {
      query
    };

    return this.connection.sendRequest(WorkspaceSymbolRequest.method, params) as Promise<WorkspaceSymbol[] | null>;
  }

  async stop(): Promise<void> {
    if (this.connection) {
      this.connection.dispose();
      this.connection = null;
    }
    if (this.process) {
      this.process.kill();
      this.process = null;
    }
    this.openedFiles.clear();
  }
}
