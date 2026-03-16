import express from 'express';
import cors from 'cors';
import { initDb } from './lib/database.js';

const app = express();

app.use(cors());
app.use(express.json());

app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    service: 'sera-core',
    timestamp: new Date().toISOString()
  });
});

app.post('/api/ingest', async (req, res) => {
  const { IngestionService } = await import('./lib/ingestion.js');
  const service = new IngestionService();
  
  // Non-blocking scan
  service.scan().catch(console.error);
  
  res.json({ status: 'started', message: 'Codebase ingestion scan initiated' });
});

export { app };

if (process.env.NODE_ENV !== 'test') {
  const port = process.env.PORT || 3001;
  initDb().then(() => {
    app.listen(port, () => {
      console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
    });
  }).catch(err => {
    console.error('Failed to start SERA Core:', err);
    process.exit(1);
  });
}
