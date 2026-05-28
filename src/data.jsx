// data.jsx — startup log and global constants

const INITIAL_LOG = [
  { ts: new Date().toLocaleTimeString('en-GB', { hour12: false }), msg: 'webclaw-software-manager 启动', tone: 'dim' },
];

Object.assign(window, { INITIAL_LOG });
