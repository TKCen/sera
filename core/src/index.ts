import express from 'express';
import cors from 'cors';
import { IngestionService } from './services/ingestion.service.js';
import { EmbeddingService } from './services/embedding.service.js';
import { VectorService } from './services/vector.service.js';

const app = express();
const port = process.env.PORT || 3001;

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
  try {
    const ingestionService = new IngestionService();
    // Non-blocking ingestion
    ingestionService.ingestCodebase().catch(err => console.error('Ingestion error:', err));
    res.json({ message: 'Ingestion started' });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/query', async (req, res) => {
  try {
    const { query, limit } = req.body;
    if (!query) {
      return res.status(400).json({ error: 'Query is required' });
    }

    const embeddingService = EmbeddingService.getInstance();
    const vectorService = new VectorService();

    const vector = await embeddingService.generateEmbedding(query);
    const results = await vectorService.search(vector, limit || 5);

    res.json({ results });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.listen(port, () => {
  console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
});
