export function ForbiddenView() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <div className="text-6xl">🚫</div>
      <h1 className="text-2xl font-semibold text-sera-text">403 — Forbidden</h1>
      <p className="max-w-md text-sm text-sera-text-muted">
        You don&apos;t have permission to access this resource.
      </p>
    </div>
  );
}
