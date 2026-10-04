import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Navigate, RouterProvider, createHashRouter } from 'react-router-dom';
import App from './components/App.jsx';
import Lines from './components/Lines.jsx';
import NewRun from './components/NewRun.jsx';
import Run from './components/Run.jsx';
import Runs from './components/Runs.jsx';
import './app.css';

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

const router = createHashRouter([
  {
    element: <App />,
    children: [
      { path: '/', element: <Runs /> },
      { path: '/new', element: <NewRun /> },
      {
        path: '/run/:id',
        element: <Run />,
        children: [
          { index: true, element: <Navigate to="scale" replace /> },
          { path: ':station', element: <Lines /> },
        ],
      },
    ],
  },
]);

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
