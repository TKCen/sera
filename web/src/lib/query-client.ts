import { QueryClient, QueryCache, MutationCache } from '@tanstack/react-query';
import { toast } from 'sonner';

export const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (_error, query) => {
      // Only show error toasts for queries that have an explicit errorMessage meta
      if (query.meta?.errorMessage) {
        toast.error(String(query.meta.errorMessage));
      }
    },
  }),
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) => {
      // Mutations show an error toast by default unless explicitly disabled
      if (mutation.meta?.showToast !== false) {
        const message = mutation.meta?.errorMessage || error.message || 'Operation failed';
        toast.error(String(message));
      }
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 2,
      refetchOnWindowFocus: false,
    },
    mutations: {
      retry: 0,
    },
  },
});
