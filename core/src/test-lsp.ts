import { LspClient } from './lsp/LspClient.js';
import path from 'path';

async function test() {
  const rootDir = process.cwd();
  const client = new LspClient({
    rootUri: `file://${rootDir}`,
    serverCommand: 'typescript-language-server',
    serverArgs: ['--stdio']
  });

  try {
    console.log('Starting LSP client...');
    await client.start();
    console.log('LSP client started.');

    const testFile = path.resolve('src/index.ts');
    const uri = `file://${testFile}`;

    console.log(`Opening ${testFile}...`);
    await client.openFile(uri, 'typescript');

    // Wait a bit for the server to analyze the file
    await new Promise(resolve => setTimeout(resolve, 1000));

    console.log(`Getting symbols for ${testFile}...`);
    const symbols = await client.getDocumentSymbols(uri);
    console.log('Symbols count:', symbols?.length);
    if (symbols && symbols.length > 0) {
        console.log('First symbol:', JSON.stringify(symbols[0], null, 2));
    } else {
        console.log('No symbols found. Full response:', JSON.stringify(symbols));
    }

  } catch (err) {
    console.error('Test failed:', err);
  } finally {
    console.log('Stopping LSP client...');
    await client.stop();
    console.log('LSP client stopped.');
  }
}

test().catch(console.error);
