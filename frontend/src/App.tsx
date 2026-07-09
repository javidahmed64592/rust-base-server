import "./App.css";

import { BrowserRouter, Navigate,Route, Routes } from "react-router-dom";

import Login from "./pages/Login";
import Protected from "./pages/Protected";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Navigate to="/protected" replace />} />
        <Route path="/login" element={<Login />} />
        <Route path="/protected" element={<Protected />} />
      </Routes>
    </BrowserRouter>
  );
}
