import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createHashRouter, RouterProvider, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { isLoggedIn } from './auth.js';
import Layout from './components/Layout.jsx';
import Login from './components/Login.jsx';
import Home from './components/Home.jsx';
import ProjectDetail from './components/ProjectDetail.jsx';
import SiteDetail from './components/SiteDetail.jsx';
import DatasetDetail from './components/DatasetDetail.jsx';
import './style.css';

function RequireAuth({ children }) {
  return isLoggedIn() ? children : <Navigate to="/login" replace />;
}

const router = createHashRouter([
  { path: '/login', element: <Login /> },
  {
    path: '/',
    element: <RequireAuth><Layout /></RequireAuth>,
    children: [
      { index: true, element: <Home /> },
      { path: 'projects/:user/:project', element: <ProjectDetail /> },
      { path: 'projects/:user/:project/sites/:name', element: <SiteDetail /> },
      { path: 'projects/:user/:project/datasets/:name', element: <DatasetDetail /> },
    ],
  },
]);

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

createRoot(document.getElementById('app')).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
