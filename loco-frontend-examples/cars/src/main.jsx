import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createHashRouter, RouterProvider } from 'react-router-dom';
import { SchemaProvider } from './components/SchemaContext.jsx';
import Layout from './components/Layout.jsx';
import CollectionView from './components/CollectionView.jsx';
import Home from './components/Home.jsx';
import './style.css';

const router = createHashRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      { index: true, element: <Home /> },
      { path: 'collection/:user/:project/:name', element: <CollectionView /> },
    ],
  },
]);

createRoot(document.getElementById('app')).render(
  <StrictMode>
    <SchemaProvider>
      <RouterProvider router={router} />
    </SchemaProvider>
  </StrictMode>,
);
