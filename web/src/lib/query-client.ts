import { QueryClient } from '@tanstack/react-query';

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
        refetchOnMount: false,
        staleTime: 0,
      },
      mutations: {
        retry: false,
        // Execute once or fail now: never queue writes for network recovery.
        networkMode: 'always',
        gcTime: 0,
      },
    },
  });
}
