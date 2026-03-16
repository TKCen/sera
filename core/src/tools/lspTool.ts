import axios from 'axios';

const CORE_API_URL = process.env.CORE_API_URL || 'http://localhost:3001';

export const lspTools = {
  get_definition: async (filePath: string, line: number, character: number) => {
    try {
      const response = await axios.post(`${CORE_API_URL}/api/lsp/definition`, {
        filePath,
        line,
        character
      });
      return response.data.definition;
    } catch (error: any) {
      return { error: error.response?.data?.error || error.message };
    }
  },

  get_references: async (filePath: string, line: number, character: number) => {
    try {
      const response = await axios.post(`${CORE_API_URL}/api/lsp/references`, {
        filePath,
        line,
        character
      });
      return response.data.references;
    } catch (error: any) {
      return { error: error.response?.data?.error || error.message };
    }
  },

  get_symbols: async (filePath: string) => {
    try {
      const response = await axios.post(`${CORE_API_URL}/api/lsp/symbols`, {
        filePath
      });
      return response.data.symbols;
    } catch (error: any) {
      return { error: error.response?.data?.error || error.message };
    }
  }
};
