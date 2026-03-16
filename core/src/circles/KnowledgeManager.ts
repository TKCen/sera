import { VectorService } from '../services/vector.service.js';
import type { VectorPoint } from '../services/vector.service.js';
import type { CircleManifest } from './types.js';

/**
 * KnowledgeManager — manages per-circle Qdrant collections for knowledge scoping.
 *
 * Each circle gets its own vector collection so knowledge is isolated
 * and doesn't pollute unrelated agent contexts.
 */
export class KnowledgeManager {
  private services: Map<string, VectorService> = new Map();

  /**
   * Ensure a Qdrant collection exists for a circle's knowledge scope.
   */
  async ensureCircleCollection(circle: CircleManifest, vectorSize: number = 1536): Promise<void> {
    const collectionName = circle.knowledge?.qdrantCollection;
    if (!collectionName) {
      console.warn(`[KnowledgeManager] Circle "${circle.metadata.name}" has no knowledge config`);
      return;
    }

    const service = this.getOrCreateService(collectionName);
    await service.ensureCollection(vectorSize);
    console.log(`[KnowledgeManager] Ensured collection "${collectionName}" for circle "${circle.metadata.name}"`);
  }

  /**
   * Ingest vector points into a circle's knowledge collection.
   */
  async ingest(circleId: string, collectionName: string, points: VectorPoint[]): Promise<void> {
    const service = this.getOrCreateService(collectionName);
    await service.upsertPoints(points);
  }

  /**
   * Search within a circle's knowledge collection.
   */
  async search(collectionName: string, vector: number[], limit: number = 5) {
    const service = this.getOrCreateService(collectionName);
    return service.search(vector, limit);
  }

  /**
   * Get or create a VectorService instance for a specific collection.
   */
  private getOrCreateService(collectionName: string): VectorService {
    let service = this.services.get(collectionName);
    if (!service) {
      service = new VectorService(collectionName);
      this.services.set(collectionName, service);
    }
    return service;
  }
}
