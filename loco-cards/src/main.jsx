import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createHashRouter, RouterProvider } from 'react-router-dom';
import { AuthProvider } from './components/AuthContext.jsx';
import Layout from './components/Layout.jsx';
import Login from './components/Login.jsx';
import Home from './components/Home.jsx';
import ProjectDetail from './components/ProjectDetail.jsx';
import DeckDetail from './components/DeckDetail.jsx';
import './style.css';

const router = createHashRouter([
  {
    path: '/login',
    element: <Login />,
  },
  {
    path: '/',
    element: <Layout />,
    children: [
      { index: true, element: <Home /> },
      { path: 'project/*', element: <ProjectDetail /> },
      { path: 'deck/*', element: <DeckDetail /> },
    ],
  },
]);

createRoot(document.getElementById('app')).render(
  <StrictMode>
    <AuthProvider>
      <RouterProvider router={router} />
    </AuthProvider>
  </StrictMode>,
);
