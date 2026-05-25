import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { createHashRouter, RouterProvider, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import 'loco-ui/tokens.css';
import { isLoggedIn } from './auth.js';
import Layout from './components/Layout.jsx';
import Login from './components/Login.jsx';
import Home from './components/Home.jsx';
import ProjectDetail from './components/ProjectDetail.jsx';
import ProjectSettings from './components/ProjectSettings.jsx';
import SiteDetail from './components/SiteDetail.jsx';
import DatasetDetail from './components/DatasetDetail.jsx';
import NewProject from './components/NewProject.jsx';
import EditProject from './components/EditProject.jsx';
import NewSite from './components/NewSite.jsx';
import EditSite from './components/EditSite.jsx';
import NewDataset from './components/NewDataset.jsx';
import EditDataset from './components/EditDataset.jsx';
import NewVersion from './components/NewVersion.jsx';
import VersionDetail from './components/VersionDetail.jsx';
import NewCollection from './components/NewCollection.jsx';
import CollectionDetail from './components/CollectionDetail.jsx';
import EditCollection from './components/EditCollection.jsx';
import NewField from './components/NewField.jsx';
import FieldDetail from './components/FieldDetail.jsx';
import EditField from './components/EditField.jsx';
import NewRecord from './components/NewRecord.jsx';
import RecordDetail from './components/RecordDetail.jsx';
import EditRecord from './components/EditRecord.jsx';
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
      { path: 'projects/new', element: <NewProject /> },
      { path: 'projects/:user/:project', element: <ProjectDetail /> },
      { path: 'projects/:user/:project/settings', element: <ProjectSettings /> },
      { path: 'projects/:user/:project/edit', element: <EditProject /> },
      { path: 'projects/:user/:project/sites/new', element: <NewSite /> },
      { path: 'projects/:user/:project/sites/:name', element: <SiteDetail /> },
      { path: 'projects/:user/:project/sites/:name/edit', element: <EditSite /> },
      { path: 'projects/:user/:project/datasets/new', element: <NewDataset /> },
      { path: 'projects/:user/:project/datasets/:name', element: <DatasetDetail /> },
      { path: 'projects/:user/:project/datasets/:name/edit', element: <EditDataset /> },
      { path: 'projects/:user/:project/versions/new', element: <NewVersion /> },
      { path: 'projects/:user/:project/versions/:version', element: <VersionDetail /> },
      { path: 'projects/:user/:project/versions/:version/collections/new', element: <NewCollection /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name', element: <CollectionDetail /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/edit', element: <EditCollection /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/fields/new', element: <NewField /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/fields/:fieldName', element: <FieldDetail /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/fields/:fieldName/edit', element: <EditField /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/records/new', element: <NewRecord /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/records/:recordId', element: <RecordDetail /> },
      { path: 'projects/:user/:project/versions/:version/collections/:name/records/:recordId/edit', element: <EditRecord /> },
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
