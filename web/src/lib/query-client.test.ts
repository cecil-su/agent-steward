import { expect, it, vi } from 'vitest';
import { onlineManager } from '@tanstack/react-query';
import { createQueryClient } from './query-client';
import { useWorkspaceStore } from '../stores/workspace';

it('disables focus/reconnect refresh and automatic write retries', () => {
  const client = createQueryClient();
  expect(client.getDefaultOptions().queries).toMatchObject({ retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchOnMount: false });
  expect(client.getDefaultOptions().mutations).toMatchObject({ retry: false, networkMode: 'always' });
  client.clear();
});
it('fails an offline mutation once instead of replaying after reconnect', async () => {
  const client = createQueryClient();
  const mutationFn = vi.fn().mockRejectedValue(new Error('uncertain'));
  onlineManager.setOnline(false);
  try {
    const mutation = client.getMutationCache().build(client, { mutationFn });
    await expect(mutation.execute(undefined)).rejects.toThrow('uncertain');
    onlineManager.setOnline(true);
    await client.resumePausedMutations();
    expect(mutationFn).toHaveBeenCalledTimes(1);
  } finally { onlineManager.setOnline(true); client.clear(); }
});
it('keeps search drafts separate and clears selection when applying a new query', () => {
  const store = useWorkspaceStore;
  store.getState().reset();
  store.getState().selectTask(40);
  store.getState().setSearchInput(' new query ');
  expect(store.getState().query).toBe('');
  expect(store.getState().selectedTaskId).toBe(40);
  store.getState().applySearch();
  expect(store.getState().query).toBe('new query');
  expect(store.getState().selectedTaskId).toBeNull();
  store.getState().reset();
});
