import express from 'express';
import cors from 'cors';
import { Orchestrator } from './agents/Orchestrator.js';
import { PrimaryAgent } from './agents/PrimaryAgent.js';
import { WorkerAgent } from './agents/WorkerAgent.js';
import { MCPRegistry } from './mcp/registry.js';

const app = express();
const port = process.env.PORT || 3001;

app.use(cors());
app.use(express.json());

const orchestrator = new Orchestrator();
const mcpRegistry = MCPRegistry.getInstance();

// Register agents
orchestrator.registerAgent(new PrimaryAgent());
orchestrator.registerAgent(new WorkerAgent('Sera-Researcher', 'researcher'));

app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    service: 'sera-core',
    timestamp: new Date().toISOString()
  });
});

app.post('/api/execute', async (req, res) => {
  const { prompt } = req.body;
  try {
    const result = await orchestrator.executeTask(prompt);
    res.json({ result });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.get('/api/tools', async (req, res) => {
  try {
    const tools = await mcpRegistry.getAllTools();
    res.json({ tools });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.listen(port, () => {
  console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
});
