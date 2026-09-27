import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider, createHashRouter } from 'react-router-dom';
import App from './components/DoesNotExist.jsx';
import BatchList from './components/BatchList.jsx';
import BatchPage from './components/BatchPage.jsx';
import './app.css';

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

const router = createHashRouter([
  {
    element: <App />,
    children: [
      { path: '/', element: <BatchList /> },
      { path: '/batch/:id', element: <BatchPage /> },
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
