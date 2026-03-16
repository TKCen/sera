export interface Position {
  line: number;
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

export interface Location {
  uri: string;
  range: Range;
}

export interface SymbolInformation {
  name: string;
  kind: number;
  location: Location;
  containerName?: string;
}

export interface LspCapabilities {
  definition: boolean;
  references: boolean;
  documentSymbol: boolean;
}

export interface LspClientOptions {
  rootUri: string;
  serverCommand: string;
  serverArgs: string[];
}
