import { useParams } from 'react-router';
import { useCircle } from '@/hooks/useCircles';

export default function CircleDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data } = useCircle(id ?? '');
  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">{data?.displayName ?? id}</h1>
      </div>
      <p className="text-sm text-sera-text-muted">Circle detail — coming in Epic 13</p>
    </div>
  );
}
