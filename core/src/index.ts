import express from 'express';
import cors from 'cors';
import lspRouter, { lspManager } from './routes/lsp.js';

const app = express();
const port = process.env.PORT || 3001;

app.use(cors());
app.use(express.json());

app.use('/api/lsp', lspRouter);

app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    service: 'sera-core',
    timestamp: new Date().toISOString()
  });
});

const server = app.listen(port, () => {
  console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
});

const shutdown = async () => {
  console.log('Shutting down SERA Core...');
  await lspManager.stopAll();
  server.close(() => {
    console.log('Server closed.');
    process.exit(0);
  });
};

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);
