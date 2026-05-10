import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getMe, isLoggedIn, login, logout } from './api.js';

export { isLoggedIn };

export function useMe() {
  return useQuery({
    queryKey: ['me'],
    queryFn: getMe,
    enabled: isLoggedIn(),
    retry: false,
  });
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: login,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['me'] }),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: logout,
    onSuccess: () => qc.removeQueries({ queryKey: ['me'] }),
  });
}
