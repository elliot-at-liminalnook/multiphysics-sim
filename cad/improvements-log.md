- REST API (`robocad/api.py`, `robocad/client.py`): every GUI window serves
  http://127.0.0.1:8420 (+1 per window), requests marshalled onto the Qt
  thread so a script or agent and the person at the keyboard edit the same
  document with one undo history. Full CRUD on nodes, every `Ops` method
  by name, sketch edits, selection and view control, PNG renders from any
  direction with x-ray/wireframe/section/labels/focus, live screenshots,
  save/open/import/export, materials, GUI commands. Headless mode
  (`python -m robocad.api model.rcad`). 5 API tests; 54 total.
