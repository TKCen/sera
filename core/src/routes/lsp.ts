import { Router } from 'express';
import { LspManager } from '../lsp/LspManager.js';
import path from 'path';
import { URI } from 'vscode-uri';

const router = Router();
const rootDir = process.cwd();
const lspManager = new LspManager(rootDir);

function validatePath(filePath: string) {
  const absolutePath = path.resolve(filePath);
  if (!absolutePath.startsWith(rootDir)) {
    throw new Error('Access denied: Path outside of workspace');
  }
  return absolutePath;
}

function getLanguageId(filePath: string) {
  const ext = path.extname(filePath);
  switch (ext) {
    case '.ts':
    case '.tsx':
      return 'typescript';
    case '.js':
    case '.jsx':
      return 'javascript';
    default:
      return 'plaintext';
  }
}

/**
 * Gets definition for a symbol at a specific file location.
 * @param req Express request containing filePath, line, and character in body
 * @param res Express response
 * @returns {Promise<void>}
 */
router.post('/definition', async (req, res) => {
  try {
    const { filePath, line, character } = req.body;
    const absolutePath = validatePath(filePath);
    const client = await lspManager.getClientForFile(absolutePath);

    if (!client) {
      return res.status(400).json({ error: 'No language server for this file type' });
    }

    const uri = URI.file(absolutePath).toString();
    await client.openFile(uri, getLanguageId(absolutePath));
    const definition = await client.getDefinition(uri, line, character);
    res.json({ definition });
  } catch (error: any) {
    res.status(error.message.startsWith('Access denied') ? 403 : 500).json({ error: error.message });
  }
});

/**
 * Finds all references for a symbol at a specific file location.
 * @param req Express request containing filePath, line, and character in body
 * @param res Express response
 * @returns {Promise<void>}
 */
router.post('/references', async (req, res) => {
  try {
    const { filePath, line, character } = req.body;
    const absolutePath = validatePath(filePath);
    const client = await lspManager.getClientForFile(absolutePath);

    if (!client) {
      return res.status(400).json({ error: 'No language server for this file type' });
    }

    const uri = URI.file(absolutePath).toString();
    await client.openFile(uri, getLanguageId(absolutePath));
    const references = await client.getReferences(uri, line, character);
    res.json({ references });
  } catch (error: any) {
    res.status(error.message.startsWith('Access denied') ? 403 : 500).json({ error: error.message });
  }
});

/**
 * Returns all symbols found within a file.
 * @param req Express request containing filePath in body
 * @param res Express response
 * @returns {Promise<void>}
 */
router.post('/symbols', async (req, res) => {
  try {
    const { filePath } = req.body;
    const absolutePath = validatePath(filePath);
    const client = await lspManager.getClientForFile(absolutePath);

    if (!client) {
      return res.status(400).json({ error: 'No language server for this file type' });
    }

    const uri = URI.file(absolutePath).toString();
    await client.openFile(uri, getLanguageId(absolutePath));
    const symbols = await client.getDocumentSymbols(uri);
    res.json({ symbols });
  } catch (error: any) {
    res.status(error.message.startsWith('Access denied') ? 403 : 500).json({ error: error.message });
  }
});

export { lspManager };
export default router;
