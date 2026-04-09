import { createContext, useContext, useState, useCallback } from 'react';
import { isLoggedIn, login as apiLogin, logout as apiLogout, getMe } from '../api.js';

const AuthContext = createContext(null);

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loggedIn, setLoggedIn] = useState(isLoggedIn());

  const login = useCallback(async (username) => {
    await apiLogin(username);
    const me = await getMe();
    setUser(me);
    setLoggedIn(true);
  }, []);

  const logout = useCallback(async () => {
    await apiLogout();
    setUser(null);
    setLoggedIn(false);
  }, []);

  const fetchUser = useCallback(async () => {
    if (!isLoggedIn()) {
      setLoggedIn(false);
      return null;
    }
    try {
      const me = await getMe();
      setUser(me);
      setLoggedIn(true);
      return me;
    } catch {
      setLoggedIn(false);
      setUser(null);
      return null;
    }
  }, []);

  return (
    <AuthContext.Provider value={{ user, loggedIn, login, logout, fetchUser }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
